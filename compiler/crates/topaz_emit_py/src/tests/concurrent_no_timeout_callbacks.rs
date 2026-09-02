use super::*;

#[test]
fn emits_concurrent_no_timeout_join_record() {
    let generated = emit_source(
        r#"
function main() -> int {
    let base = 10
    let r = concurrent {
        x: base + 1
        y: base * 2
    }
    print("{r.x} {r.y}")
    0
}
main()
"#,
    );
    assert!(
        generated.contains("class _tr_78_79:"),
        "concurrent join should predeclare the arm-result record shape: {generated}"
    );
    assert!(
        generated.contains("_tr_78_79("),
        "concurrent join should construct the arm-result record: {generated}"
    );
    assert!(
        generated.contains("# concurrent x") && generated.contains("# concurrent y"),
        "concurrent arms should keep source comments on their thunks: {generated}"
    );
    assert!(
        generated.contains("tpz_concurrent_join(["),
        "no-timeout concurrent should use the cooperative Python scheduler: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("concurrent no-timeout join Python gate failed: {e}"));
}

#[test]
fn emits_concurrent_no_timeout_arm_while_checkpoint() {
    let generated = emit_source(
        r#"
function main() -> int {
    concurrent {
        slow: {
            let mut i = 0
            while i < 2 {
                i = i + 1
            }
            1
        }
        fast: 2
    }
    0
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_concurrent_join(["),
        "no-timeout concurrent should run through the scheduler: {generated}"
    );
    assert!(
        generated.contains("yield None"),
        "long-running concurrent arms should yield at loop back-edges: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("concurrent while checkpoint Python gate failed: {e}"));
}

#[test]
fn emits_concurrent_no_timeout_nested_function_without_yield_leak() {
    let generated = emit_source(
        r#"
function main() -> int {
    let r = concurrent {
        a: {
            function helper() -> int {
                let mut i = 0
                while i < 3 {
                    i = i + 1
                }
                i
            }
            helper()
        }
        b: 5
    }
    r.a + r.b
}
main()
"#,
    );
    let helper_py_name = "_t_68656c706572";
    let helper_start = generated
        .find(&format!("def {helper_py_name}"))
        .unwrap_or_else(|| panic!("nested helper should emit as a Python def: {generated}"));
    let helper_body_start = generated[helper_start..]
        .find('\n')
        .map(|offset| helper_start + offset + 1)
        .unwrap_or_else(|| panic!("nested helper def should have a body: {generated}"));
    let helper_co_py_name = "_t_68656c706572__co";
    let helper_co_start = generated[helper_body_start..]
        .find(&format!("def {helper_co_py_name}"))
        .map(|offset| helper_body_start + offset)
        .unwrap_or_else(|| {
            panic!("nested helper should emit a cooperative generator variant: {generated}")
        });
    let helper_call = generated[helper_body_start..]
        .find(&format!("yield from {helper_co_py_name}()"))
        .map(|offset| helper_body_start + offset)
        .unwrap_or_else(|| {
            panic!("nested helper call should use the cooperative variant: {generated}")
        });
    let helper_region = &generated[helper_start..helper_co_start];
    assert!(
        !helper_region.contains("yield None"),
        "plain nested helper def must not become a generator: {generated}"
    );
    let helper_co_region = &generated[helper_co_start..helper_call];
    assert!(
        helper_co_region.contains("yield None"),
        "cooperative nested helper variant should yield at loop checkpoints: {generated}"
    );
    assert_generated_python_ok_int(&generated, 8, "concurrent nested helper loop call");
}

#[test]
fn emits_concurrent_no_timeout_function_value_hof_callback_aliases() {
    let top_level_alias = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let alias = spin

concurrent {
    slow: {
        let xs = [1]
        xs.map(alias)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        top_level_alias.contains("yield from tpz_array_map__co(")
            && top_level_alias.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "top-level function-value HOF callback alias should route through the cooperative target: {top_level_alias}"
    );
    assert_generated_python_gates(&top_level_alias).unwrap_or_else(|e| {
        panic!("top-level function-value HOF callback alias Python gate failed: {e}")
    });

    let alias_chain = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let first = spin
let second = first

concurrent {
    slow: {
        let xs = [1]
        xs.map(second)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        alias_chain.contains("yield from tpz_array_map__co(")
            && alias_chain.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "function-value HOF callback alias chains should preserve the cooperative target: {alias_chain}"
    );
    assert_generated_python_gates(&alias_chain).unwrap_or_else(|e| {
        panic!("function-value HOF callback alias chain Python gate failed: {e}")
    });

    let nested_alias = emit_source(
        r#"
function main() -> int {
    concurrent {
        slow: {
            function spin(x: int) -> int {
                let mut i = 0
                while i < 3 {
                    i = i + 1
                }
                x
            }
            let alias = spin
            let xs = [1]
            xs.map(alias)
        }
        fast: 0
    }
    0
}
main()
"#,
    );
    assert!(
        nested_alias.contains("yield from tpz_array_map__co(")
            && nested_alias.contains("_t_7370696e__co(__tpz_cb_0)"),
        "nested function-value HOF callback aliases should route through the nested cooperative target without host: {nested_alias}"
    );
    assert_generated_python_gates(&nested_alias).unwrap_or_else(|e| {
        panic!("nested function-value HOF callback alias Python gate failed: {e}")
    });
}

#[test]
fn emits_concurrent_no_timeout_static_array_index_function_value_hof_callbacks() {
    let direct_element = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let callbacks = [spin]

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[0])
    }
    fast: 0
}
0
"#,
    );
    assert!(
        direct_element.contains("yield from tpz_array_map__co(")
            && direct_element.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !direct_element.contains("tpz_index(_t_63616c6c6261636b73, 0,"),
        "static array-index function-value HOF callback should route through the cooperative target without a runtime callback read: {direct_element}"
    );
    assert_generated_python_gates(&direct_element).unwrap_or_else(|e| {
        panic!("static array-index function-value HOF callback Python gate failed: {e}")
    });

    let alias_element = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let alias = spin
let callbacks = [alias]

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[0])
    }
    fast: 0
}
0
"#,
    );
    assert!(
        alias_element.contains("yield from tpz_array_map__co(")
            && alias_element.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !alias_element.contains("tpz_index(_t_63616c6c6261636b73, 0,"),
        "static array-index function-value alias elements should preserve the cooperative target: {alias_element}"
    );
    assert_generated_python_gates(&alias_element)
        .unwrap_or_else(|e| panic!("static array-index alias callback Python gate failed: {e}"));

    let spread_element = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let base = [spin]
let callbacks = [...base, spin]

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[1])
    }
    fast: 0
}
0
"#,
    );
    assert!(
        spread_element.contains("yield from tpz_array_map__co(")
            && spread_element.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !spread_element.contains("tpz_index(_t_63616c6c6261636b73, 1,"),
        "static spread-array function-value HOF callback should route through the cooperative target: {spread_element}"
    );
    assert_generated_python_gates(&spread_element)
        .unwrap_or_else(|e| panic!("static spread-array callback Python gate failed: {e}"));

    let nested_spread_element = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let base = [spin]
let first = [...base, spin]
let callbacks = [...first, spin]

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[2])
    }
    fast: 0
}
0
"#,
    );
    assert!(
        nested_spread_element.contains("yield from tpz_array_map__co(")
            && nested_spread_element.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !nested_spread_element.contains("tpz_index(_t_63616c6c6261636b73, 2,"),
        "nested static spread-array function-value HOF callback should preserve cooperative metadata: {nested_spread_element}"
    );
    assert_generated_python_gates(&nested_spread_element)
        .unwrap_or_else(|e| panic!("nested static spread-array callback Python gate failed: {e}"));

    let mutable_element = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let mut callbacks = [spin]

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[0])
    }
    fast: 0
}
0
"#,
    );
    assert!(
        mutable_element.contains("yield from tpz_array_map__co(")
            && mutable_element.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !mutable_element.contains("tpz_index(_t_63616c6c6261636b73, 0,"),
        "mutable static array-index function-value HOF callback should route through the cooperative target while the slot is unmutated: {mutable_element}"
    );
    assert_generated_python_gates(&mutable_element)
        .unwrap_or_else(|e| panic!("mutable static array-index callback Python gate failed: {e}"));

    let mutable_alias_element = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let alias = spin
let mut callbacks = [alias]

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[0])
    }
    fast: 0
}
0
"#,
    );
    assert!(
        mutable_alias_element.contains("yield from tpz_array_map__co(")
            && mutable_alias_element.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !mutable_alias_element.contains("tpz_index(_t_63616c6c6261636b73, 0,"),
        "mutable static array-index immutable alias elements should preserve the cooperative target until the slot changes: {mutable_alias_element}"
    );
    assert_generated_python_gates(&mutable_alias_element).unwrap_or_else(|e| {
        panic!("mutable static array-index alias callback Python gate failed: {e}")
    });

    let mutable_static_reassign = emit_source(
        r#"
function spin(x: int) -> int {
    x
}

function spin2(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let mut callbacks = [spin]
callbacks[0] = spin2

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[0])
    }
    fast: 0
}
0
"#,
    );
    assert!(
        mutable_static_reassign.contains("yield from tpz_array_map__co(")
            && mutable_static_reassign.contains("_t_7370696e32__co(host, __tpz_cb_0)")
            && !mutable_static_reassign.contains("tpz_index(_t_63616c6c6261636b73, 0,"),
        "mutable static array-index reassignment should refresh the cooperative target instead of using stale metadata: {mutable_static_reassign}"
    );
    assert_generated_python_gates(&mutable_static_reassign).unwrap_or_else(|e| {
        panic!("mutable static array-index reassignment Python gate failed: {e}")
    });
}

#[test]
fn emits_concurrent_no_timeout_partial_spread_array_function_value_hof_callbacks() {
    let partial_literal_element = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let callbacks = [42, spin]

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[1])
    }
    fast: 0
}
0
"#,
    );
    assert!(
        partial_literal_element.contains("yield from tpz_array_map__co(")
            && partial_literal_element.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !partial_literal_element.contains("tpz_index(_t_63616c6c6261636b73, 1,"),
        "partial literal callback arrays should promote only proven static slots: {partial_literal_element}"
    );
    assert_generated_python_gates(&partial_literal_element)
        .unwrap_or_else(|e| panic!("partial literal callback array Python gate failed: {e}"));

    let partial_spread_element = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let base = [42]
let callbacks = [...base, spin]

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[1])
    }
    fast: 0
}
0
"#,
    );
    assert!(
        partial_spread_element.contains("yield from tpz_array_map__co(")
            && partial_spread_element.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !partial_spread_element.contains("tpz_index(_t_63616c6c6261636b73, 1,"),
        "partial spread-built callback arrays should preserve cooperative metadata for proven slots: {partial_spread_element}"
    );
    assert_generated_python_gates(&partial_spread_element)
        .unwrap_or_else(|e| panic!("partial spread-built callback array Python gate failed: {e}"));

    let nested_partial_spread_element = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let base = [42]
let first = [...base, spin]
let callbacks = [...first, spin]

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[2])
    }
    fast: 0
}
0
"#,
    );
    assert!(
        nested_partial_spread_element.contains("yield from tpz_array_map__co(")
            && nested_partial_spread_element.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !nested_partial_spread_element.contains("tpz_index(_t_63616c6c6261636b73, 2,"),
        "nested partial spread-built callback arrays should preserve slot metadata: {nested_partial_spread_element}"
    );
    assert_generated_python_gates(&nested_partial_spread_element).unwrap_or_else(|e| {
        panic!("nested partial spread-built callback array Python gate failed: {e}")
    });

    let partial_lambda_spread_element = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let cb = (x) => spin(x)
let base = [42]
let callbacks = [...base, cb]

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks[1])
    }
    fast: 0
}
0
"#,
    );
    assert!(
        partial_lambda_spread_element.contains("yield from tpz_array_map__co(")
            && partial_lambda_spread_element.contains("_t_6362__co(__tpz_cb_0)")
            && !partial_lambda_spread_element.contains("tpz_index(_t_63616c6c6261636b73, 1,"),
        "partial spread-built callback arrays should preserve value-bound lambda cooperative metadata: {partial_lambda_spread_element}"
    );
    assert_generated_python_gates(&partial_lambda_spread_element).unwrap_or_else(|e| {
        panic!("partial lambda spread-built callback array Python gate failed: {e}")
    });
}

#[test]
fn emits_concurrent_no_timeout_record_field_function_value_hof_callbacks() {
    let direct_field = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let callbacks = { primary: spin }

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks.primary)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        direct_field.contains("yield from tpz_array_map__co(")
            && direct_field.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !direct_field.contains("tpz_member(_t_63616c6c6261636b73, \"_t_7072696d617279\""),
        "record-field function-value HOF callback should route through the cooperative target without a runtime member read: {direct_field}"
    );
    assert_generated_python_gates(&direct_field).unwrap_or_else(|e| {
        panic!("record-field function-value HOF callback Python gate failed: {e}")
    });

    let alias_field = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let alias = spin
let callbacks = { primary: alias }

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks.primary)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        alias_field.contains("yield from tpz_array_map__co(")
            && alias_field.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !alias_field.contains("tpz_member(_t_63616c6c6261636b73, \"_t_7072696d617279\""),
        "record-field function-value aliases should preserve the cooperative target: {alias_field}"
    );
    assert_generated_python_gates(&alias_field)
        .unwrap_or_else(|e| panic!("record-field alias callback Python gate failed: {e}"));

    let record_alias = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let first = { primary: spin }
let callbacks = first

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks.primary)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        record_alias.contains("yield from tpz_array_map__co(")
            && record_alias.contains("_t_7370696e__co(host, __tpz_cb_0)"),
        "immutable record aliases should preserve field cooperative callback metadata: {record_alias}"
    );
    assert_generated_python_gates(&record_alias)
        .unwrap_or_else(|e| panic!("record-alias field callback Python gate failed: {e}"));

    let mutable_direct_field = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let mut callbacks = { primary: spin }

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks.primary)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        mutable_direct_field.contains("yield from tpz_array_map__co(")
            && mutable_direct_field.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !mutable_direct_field
                .contains("tpz_member(_t_63616c6c6261636b73, \"_t_7072696d617279\""),
        "unmutated mutable record-field function-value HOF callback should route through the cooperative target: {mutable_direct_field}"
    );
    assert_generated_python_gates(&mutable_direct_field)
        .unwrap_or_else(|e| panic!("mutable record-field callback Python gate failed: {e}"));

    let mutable_alias_field = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let alias = spin
let mut callbacks = { primary: alias }

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks.primary)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        mutable_alias_field.contains("yield from tpz_array_map__co(")
            && mutable_alias_field.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !mutable_alias_field
                .contains("tpz_member(_t_63616c6c6261636b73, \"_t_7072696d617279\""),
        "mutable record-field immutable aliases should preserve the cooperative target until the field changes: {mutable_alias_field}"
    );
    assert_generated_python_gates(&mutable_alias_field)
        .unwrap_or_else(|e| panic!("mutable record-field alias callback Python gate failed: {e}"));

    let mutable_record_reassign = emit_source(
        r#"
function quick(x: int) -> int {
    x
}

function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let mut callbacks = { primary: quick }
callbacks.primary = spin

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks.primary)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        mutable_record_reassign.contains("yield from tpz_array_map__co(")
            && mutable_record_reassign.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !mutable_record_reassign
                .contains("tpz_member(_t_63616c6c6261636b73, \"_t_7072696d617279\""),
        "mutable record-field reassignment should refresh the cooperative target instead of using stale metadata: {mutable_record_reassign}"
    );
    assert_generated_python_gates(&mutable_record_reassign)
        .unwrap_or_else(|e| panic!("mutable record-field reassignment Python gate failed: {e}"));

    let mutable_nested_field = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let mut callbacks = { nested: { primary: spin } }

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks.nested.primary)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        mutable_nested_field.contains("yield from tpz_array_map__co(")
            && mutable_nested_field.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !mutable_nested_field
                .contains("tpz_member(_t_63616c6c6261636b73, \"_t_6e6573746564\"")
            && !mutable_nested_field.contains("tpz_member(tpz_member("),
        "unmutated mutable nested record-field function-value HOF callback should route through the cooperative target without runtime member reads: {mutable_nested_field}"
    );
    assert_generated_python_gates(&mutable_nested_field)
        .unwrap_or_else(|e| panic!("mutable nested record-field callback Python gate failed: {e}"));

    let mutable_nested_field_inside_loop = emit_source(
        r#"
function one(x: int) -> int {
    1
}
function two(x: int) -> int {
    2
}
function main() -> int {
    let mut inner = { primary: one }
    let mut i = 0
    let mut total = 0
    while i < 2 {
        let result = concurrent {
            a: {
                let xs = [1]
                let ys = xs.map(inner.primary)
                ys[0]
            }
            b: 0
        }
        total = total + result.a
        inner.primary = two
        i = i + 1
    }
    total
}
main()
"#,
    );
    assert!(
        mutable_nested_field_inside_loop.contains("yield from tpz_array_map__co(")
            && mutable_nested_field_inside_loop
                .contains("tpz_member(_t_696e6e6572, \"_t_7072696d617279\"")
            && !mutable_nested_field_inside_loop.contains("_t_6f6e65__co(host, __tpz_cb_0)")
            && !mutable_nested_field_inside_loop.contains("_t_74776f__co(host, __tpz_cb_0)"),
        "mutable record-field callbacks inside loop-carried mutation must read the runtime member instead of baking stale cooperative metadata: {mutable_nested_field_inside_loop}"
    );
    assert_generated_python_ok_int(
        &mutable_nested_field_inside_loop,
        3,
        "loop-carried mutable record-field callback metadata",
    );

    let nested_field = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let callbacks = { nested: { primary: spin } }

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks.nested.primary)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        nested_field.contains("yield from tpz_array_map__co(")
            && nested_field.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !nested_field.contains("tpz_member(_t_63616c6c6261636b73, \"_t_6e6573746564\"")
            && !nested_field.contains("tpz_member(tpz_member("),
        "nested record-field function-value HOF callback should route through the cooperative target without runtime member reads: {nested_field}"
    );
    assert_generated_python_gates(&nested_field)
        .unwrap_or_else(|e| panic!("nested record-field callback Python gate failed: {e}"));

    let nested_alias_field = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let alias = spin
let inner = { primary: alias }
let callbacks = { nested: inner }

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks.nested.primary)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        nested_alias_field.contains("yield from tpz_array_map__co(")
            && nested_alias_field.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !nested_alias_field
                .contains("tpz_member(_t_63616c6c6261636b73, \"_t_6e6573746564\"")
            && !nested_alias_field.contains("tpz_member(tpz_member("),
        "nested immutable record aliases should preserve field cooperative callback metadata: {nested_alias_field}"
    );
    assert_generated_python_gates(&nested_alias_field)
        .unwrap_or_else(|e| panic!("nested record-alias field callback Python gate failed: {e}"));

    let nested_record_alias = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let inner = { primary: spin }
let first = { nested: inner }
let callbacks = first

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks.nested.primary)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        nested_record_alias.contains("yield from tpz_array_map__co(")
            && nested_record_alias.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && !nested_record_alias
                .contains("tpz_member(_t_63616c6c6261636b73, \"_t_6e6573746564\"")
            && !nested_record_alias.contains("tpz_member(tpz_member("),
        "nested immutable record-alias chains should preserve field cooperative callback metadata: {nested_record_alias}"
    );
    assert_generated_python_gates(&nested_record_alias)
        .unwrap_or_else(|e| panic!("nested record-alias chain callback Python gate failed: {e}"));

    let mutable_inner_record_alias_snapshot = emit_source(
        r#"
function quick(x: int) -> int {
    x
}

function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let mut inner = { primary: spin }
let callbacks = { nested: inner }
inner.primary = quick

concurrent {
    slow: {
        let xs = [1]
        xs.map(callbacks.nested.primary)
    }
    fast: 0
}
concurrent {
    slow: {
        let xs = [1]
        xs.map(inner.primary)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        mutable_inner_record_alias_snapshot.contains("yield from tpz_array_map__co(")
            && mutable_inner_record_alias_snapshot.contains("_t_7370696e__co(host, __tpz_cb_0)")
            && mutable_inner_record_alias_snapshot.contains("_t_717569636b__co(host, __tpz_cb_0)")
            && !mutable_inner_record_alias_snapshot
                .contains("tpz_member(_t_63616c6c6261636b73, \"_t_6e6573746564\"")
            && !mutable_inner_record_alias_snapshot.contains("tpz_member(tpz_member("),
        "mutable inner record-alias snapshots should preserve alias-time nested cooperative metadata while later direct field reassignment refreshes the original mutable binding: {mutable_inner_record_alias_snapshot}"
    );
    assert_generated_python_gates(&mutable_inner_record_alias_snapshot).unwrap_or_else(|e| {
        panic!("mutable inner record-alias snapshot callback Python gate failed: {e}")
    });
}

#[test]
fn emits_concurrent_no_timeout_composed_hof_callbacks() {
    let top_level_compose = emit_source(
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

let pipeline = spin >> same

concurrent {
    slow: {
        let xs = [1]
        xs.map(pipeline)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        top_level_compose.contains("yield from tpz_array_map__co(")
            && top_level_compose.contains(".__call_cooperative__(__tpz_cb_0)")
            && top_level_compose.contains("tpz_host_callable(_t_7370696e, host, _t_7370696e__co)"),
        "top-level composed HOF callbacks should route through cooperative compose metadata: {top_level_compose}"
    );
    assert_generated_python_gates(&top_level_compose)
        .unwrap_or_else(|e| panic!("top-level composed HOF callback Python gate failed: {e}"));

    let alias_chain = emit_source(
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

let first = spin >> same
let second = first

concurrent {
    slow: {
        let xs = [1]
        xs.map(second)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        alias_chain.contains("yield from tpz_array_map__co(")
            && alias_chain.contains(".__call_cooperative__(__tpz_cb_0)"),
        "composed HOF callback alias chains should preserve the cooperative compose adapter: {alias_chain}"
    );
    assert_generated_python_gates(&alias_chain)
        .unwrap_or_else(|e| panic!("composed HOF callback alias chain Python gate failed: {e}"));

    let second_operand = emit_source(
        r#"
function same(x: int) -> int {
    x
}
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    1 / 0
}

let pipeline = same >> spin

concurrent {
    slow: {
        let xs = [1]
        xs.map(pipeline)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        second_operand.contains("yield from tpz_array_map__co(")
            && second_operand.contains(".__call_cooperative__(__tpz_cb_0)")
            && second_operand.contains("tpz_host_callable(_t_7370696e, host, _t_7370696e__co)"),
        "composed HOF callbacks should yield when only the second operand needs the scheduler: {second_operand}"
    );
    assert_generated_python_gates(&second_operand)
        .unwrap_or_else(|e| panic!("second-operand composed HOF callback Python gate failed: {e}"));

    let mutable_compose = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
function same(x: int) -> int {
    x
}

let mut pipeline = spin >> same

concurrent {
    slow: {
        let xs = [1]
        xs.map(pipeline)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        mutable_compose.contains("yield from tpz_array_map__co(")
            && mutable_compose.contains(".__call_cooperative__(__tpz_cb_0)")
            && mutable_compose.contains(
                "(lambda __tpz_target: (lambda __tpz_cb_0: __tpz_target.__call_cooperative__(__tpz_cb_0)))"
            )
            && mutable_compose
                .contains("tpz_host_callable(_t_7370696e, host, _t_7370696e__co)"),
        "unmutated mutable composed HOF callbacks should preserve cooperative compose metadata: {mutable_compose}"
    );
    assert_generated_python_gates(&mutable_compose)
        .unwrap_or_else(|e| panic!("mutable composed HOF callback Python gate failed: {e}"));

    let mutable_compose_cross_arm_reassignment = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 200 {
        i = i + 1
    }
    x
}
function same(x: int) -> int {
    x
}
function main() -> int {
    let mut pipeline = spin >> same
    let r = concurrent {
        slow: {
            let xs = [1, 2]
            let ys = xs.map(pipeline)
            ys[0] + ys[1]
        }
        flip: {
            pipeline = (x) => 9
            0
        }
    }
    r.slow
}
main()
"#,
    );
    assert!(
        mutable_compose_cross_arm_reassignment.contains("yield from tpz_array_map__co(")
            && !mutable_compose_cross_arm_reassignment
                .contains(".__call_cooperative__(__tpz_cb_0)"),
        "concurrent arms with cross-arm writers should use the runtime cooperative driver without baking mutable composed callback metadata: {mutable_compose_cross_arm_reassignment}"
    );
    assert_generated_python_ok_int(
        &mutable_compose_cross_arm_reassignment,
        3,
        "mutable composed callback cross-arm reassignment",
    );

    let mutable_compose_cross_arm_reassignment_after_yield = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 200 {
        i = i + 1
    }
    x
}
function same(x: int) -> int {
    x
}
function main() -> int {
    let mut pipeline = spin >> same
    let r = concurrent {
        slow: {
            let warm = spin(1)
            let xs = [1, 2]
            let ys = xs.map(pipeline)
            warm + ys[0] + ys[1]
        }
        flip: {
            pipeline = (x) => 9
            0
        }
    }
    r.slow
}
main()
"#,
    );
    assert!(
        mutable_compose_cross_arm_reassignment_after_yield
            .contains("yield from tpz_array_map__co(")
            && !mutable_compose_cross_arm_reassignment_after_yield
                .contains(".__call_cooperative__(__tpz_cb_0)"),
        "concurrent arms with pre-map yields and cross-arm writers must read the callback at runtime: {mutable_compose_cross_arm_reassignment_after_yield}"
    );
    assert_generated_python_ok_int(
        &mutable_compose_cross_arm_reassignment_after_yield,
        19,
        "mutable composed callback cross-arm reassignment after yield",
    );

    let mutable_compose_after_plain_reassignment = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}
function same(x: int) -> int {
    x
}

let mut pipeline = spin >> same
pipeline = (x) => x

concurrent {
    slow: {
        let xs = [1]
        xs.map(pipeline)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        mutable_compose_after_plain_reassignment.contains("yield from tpz_array_map__co(")
            && !mutable_compose_after_plain_reassignment.contains(".__call_cooperative__("),
        "plain reassignment should clear stale mutable composed callback metadata while keeping the runtime cooperative driver: {mutable_compose_after_plain_reassignment}"
    );
    assert_generated_python_gates(&mutable_compose_after_plain_reassignment).unwrap_or_else(|e| {
        panic!("mutable composed HOF callback reassignment Python gate failed: {e}")
    });

    let mutable_compose_inside_loop = emit_source(
        r#"
function one(x: int) -> int {
    1
}
function same(x: int) -> int {
    x
}
function main() -> int {
    let mut pipeline = one >> same
    let mut i = 0
    let mut total = 0
    while i < 2 {
        let result = concurrent {
            slow: {
                let xs = [1]
                let ys = xs.map(pipeline)
                ys[0]
            }
            fast: 0
        }
        total = total + result.slow
        pipeline = (x) => 2
        i = i + 1
    }
    total
}
main()
"#,
    );
    assert!(
        mutable_compose_inside_loop.contains("yield from tpz_array_map__co(")
            && !mutable_compose_inside_loop.contains(".__call_cooperative__(__tpz_cb_0)"),
        "mutable composed callbacks inside loop-carried mutation should use the runtime cooperative driver without baking a stale compose adapter: {mutable_compose_inside_loop}"
    );
    assert_generated_python_ok_int(
        &mutable_compose_inside_loop,
        3,
        "loop-carried mutable composed callback metadata",
    );
}
