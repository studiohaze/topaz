use super::*;

#[test]
fn emits_concurrent_no_timeout_value_bound_lambda_hof_callback_aliases() {
    let top_level_lambda = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let cb = (x) => spin(x)

concurrent {
    slow: {
        let xs = [1]
        xs.map(cb)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        top_level_lambda.contains("globals()[\"_t_6362__co\"] = (lambda")
            && top_level_lambda.contains("_t_7370696e__co(host, _t_78)")
            && top_level_lambda.contains("_t_6362__co(__tpz_cb_0)")
            && !top_level_lambda.contains("_t_6362__co(host"),
        "top-level value-bound lambda HOF callbacks should route through a def-site cooperative sibling without host injection: {top_level_lambda}"
    );
    assert_generated_python_gates(&top_level_lambda).unwrap_or_else(|e| {
        panic!("top-level value-bound lambda HOF callback Python gate failed: {e}")
    });

    let paren_lambda = emit_source(
        r#"
function spin(x: int) -> int {
    let mut i = 0
    while i < 3 {
        i = i + 1
    }
    x
}

let cb = ((x) => spin(x))

concurrent {
    slow: {
        let xs = [1]
        xs.map(cb)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        paren_lambda.contains("globals()[\"_t_6362__co\"] = (")
            && paren_lambda.contains("_t_7370696e__co(host, _t_78)")
            && paren_lambda.contains("_t_6362__co(__tpz_cb_0)"),
        "parenthesized value-bound lambda HOF callbacks should preserve the cooperative sibling: {paren_lambda}"
    );
    assert_generated_python_gates(&paren_lambda).unwrap_or_else(|e| {
        panic!("parenthesized value-bound lambda HOF callback Python gate failed: {e}")
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

let cb = (x) => spin(x)
let next = cb

concurrent {
    slow: {
        let xs = [1]
        xs.map(next)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        alias_chain.contains("globals()[\"_t_6362__co\"] = (lambda")
            && alias_chain.contains("_t_6362__co(__tpz_cb_0)"),
        "value-bound lambda alias chains should preserve the def-site cooperative sibling: {alias_chain}"
    );
    assert_generated_python_gates(&alias_chain)
        .unwrap_or_else(|e| panic!("value-bound lambda alias chain Python gate failed: {e}"));

    let nested_lambda = emit_source(
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
            let cb = (x) => spin(x)
            let xs = [1]
            xs.map(cb)
        }
        fast: 0
    }
    0
}
main()
"#,
    );
    assert!(
        nested_lambda.contains("_t_6362__co = (lambda")
            && nested_lambda.contains("_t_7370696e__co(_t_78)")
            && nested_lambda.contains("_t_6362__co(__tpz_cb_0)")
            && !nested_lambda.contains("_t_6362__co(host"),
        "nested value-bound lambda HOF callbacks should use the nested cooperative sibling without host: {nested_lambda}"
    );
    assert_generated_python_gates(&nested_lambda).unwrap_or_else(|e| {
        panic!("nested value-bound lambda HOF callback Python gate failed: {e}")
    });
}

#[test]
fn routes_mutable_and_shadowed_function_value_callbacks_through_runtime_driver() {
    let mutable_alias = emit_source(
        r#"
function spin(x: int) -> int {
    x
}

let mut alias = spin

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
        mutable_alias.contains("yield from tpz_array_map__co("),
        "mutable function-value aliases should use the runtime cooperative HOF driver: {mutable_alias}"
    );
    assert_generated_python_gates(&mutable_alias).unwrap_or_else(|e| {
        panic!("mutable function-value HOF callback alias Python gate failed: {e}")
    });

    let shadowed_name = emit_source(
        r#"
function spin(x: int) -> int {
    x
}

function main() -> int {
    let spin = 1
    concurrent {
        slow: {
            let xs = [1]
            xs.map(spin)
        }
        fast: 0
    }
    0
}
main()
"#,
    );
    assert!(
        shadowed_name.contains("yield from tpz_array_map__co("),
        "local shadows of known functions should still use the runtime cooperative HOF driver: {shadowed_name}"
    );
    assert_generated_python_gates(&shadowed_name)
        .unwrap_or_else(|e| panic!("shadowed function-value HOF callback Python gate failed: {e}"));
}

#[test]
fn routes_unproven_value_bound_lambda_hof_callbacks_through_runtime_driver() {
    let mutable_lambda = emit_source(
        r#"
function spin(x: int) -> int {
    x
}

let mut cb = (x) => spin(x)

concurrent {
    slow: {
        let xs = [1]
        xs.map(cb)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        mutable_lambda.contains("yield from tpz_array_map__co(")
            && !mutable_lambda.contains("_t_6362__co"),
        "mutable value-bound lambda HOF callbacks should use the runtime driver without synthesizing an unproven cooperative sibling: {mutable_lambda}"
    );
    assert_generated_python_gates(&mutable_lambda).unwrap_or_else(|e| {
        panic!("mutable value-bound lambda HOF callback Python gate failed: {e}")
    });

    let plain_lambda = emit_source(
        r#"
let cb = (x) => x + 1

concurrent {
    slow: {
        let xs = [1]
        xs.map(cb)
    }
    fast: 0
}
0
"#,
    );
    assert!(
        plain_lambda.contains("yield from tpz_array_map__co(")
            && !plain_lambda.contains("_t_6362__co"),
        "value-bound lambdas without cooperative known calls should use the runtime driver without a cooperative sibling: {plain_lambda}"
    );
    assert_generated_python_gates(&plain_lambda).unwrap_or_else(|e| {
        panic!("plain value-bound lambda HOF callback Python gate failed: {e}")
    });

    let shadowed_callee = emit_source(
        r#"
function spin(x: int) -> int {
    x
}

function main() -> int {
    let spin = (x) => x
    let cb = (x) => spin(x)
    concurrent {
        slow: {
            let xs = [1]
            xs.map(cb)
        }
        fast: 0
    }
    0
}
main()
"#,
    );
    assert!(
        shadowed_callee.contains("yield from tpz_array_map__co(")
            && !shadowed_callee.contains("_t_6362__co"),
        "value-bound lambdas should respect def-site callee shadows while using the runtime driver: {shadowed_callee}"
    );
    assert_generated_python_gates(&shadowed_callee).unwrap_or_else(|e| {
        panic!("shadowed value-bound lambda HOF callback Python gate failed: {e}")
    });
}

#[test]
fn array_mutation_metadata_observation_traverses_expression_and_statement_containers() {
    let expression_containers = emit_source(
        r#"
function spin(x: int) -> int {
    x
}

function keepUnit(value: unit) -> unit {
    value
}

let mut stringCallbacks = [spin]
let rendered = "cleared {stringCallbacks.clear()}"

let mut pipeCallbacks = [spin]
let piped = pipeCallbacks.clear() |> keepUnit

let mut comprehensionCallbacks = [spin]
let collected = [for ignored in [0] => comprehensionCallbacks.clear()]

concurrent {
    slow: {
        let xs = [1]
        xs.map(stringCallbacks[0])
        xs.map(pipeCallbacks[0])
        xs.map(comprehensionCallbacks[0])
    }
    fast: 0
}
0
"#,
    );
    assert!(
        expression_containers
            .matches("yield from tpz_array_map__co(")
            .count()
            >= 3
            && !expression_containers.contains("_t_7370696e__co(host, __tpz_cb_"),
        "string, pipe, and comprehension mutators should invalidate static callback metadata and keep the runtime driver: {expression_containers}"
    );
    assert_generated_python_gates(&expression_containers).unwrap_or_else(|e| {
        panic!("expression-container mutator callback array Python gate failed: {e}")
    });

    let statement_containers = emit_source(
        r#"
function spin(x: int) -> int {
    x
}

function getFile() -> File {
    match open("config.txt") {
        case Ok(file) => file
        case Err(_) => loop {}
    }
}

function main() -> int {
    let mut targetCallbacks = [spin]
    let mut slots = [0]
    slots[{
        targetCallbacks.clear()
        0
    }] = 1

    let mut whileCallbacks = [spin]
    while {
        whileCallbacks.clear()
        false
    } {}

    let mut usingCallbacks = [spin]
    using file = {
        usingCallbacks.clear()
        getFile()
    } {}

    let mut breakCallbacks = [spin]
    let ignored = loop {
        break {
            breakCallbacks.clear()
            ()
        }
    }

    concurrent {
        slow: {
            let xs = [1]
            xs.map(targetCallbacks[0])
            xs.map(whileCallbacks[0])
            xs.map(usingCallbacks[0])
            xs.map(breakCallbacks[0])
        }
        fast: 0
    }
    0
}
main()
"#,
    );
    assert!(
        statement_containers
            .matches("yield from tpz_array_map__co(")
            .count()
            >= 4
            && !statement_containers.contains("_t_7370696e__co(host, __tpz_cb_"),
        "assignment-target, while, using, and break-value mutators should invalidate static callback metadata and keep the runtime driver: {statement_containers}"
    );
    assert_generated_python_gates(&statement_containers).unwrap_or_else(|e| {
        panic!("statement-container mutator callback array Python gate failed: {e}")
    });
}
