use super::*;

#[test]
fn emits_nested_function_with_immutable_capture() {
    let generated = emit_source(
        r#"
function main() -> int {
    let base = 10
    function addBase(x: int) -> int {
        base + x
    }
    addBase(4)
}
main()
"#,
    );
    assert!(
        generated.contains("    def _t_61646442617365(_t_78):  # addBase(x)"),
        "nested function should lower to a local Python def without host parameter: {generated}"
    );
    assert!(
        generated.contains("_t_61646442617365(4)"),
        "nested function call should use the local callable path: {generated}"
    );
    assert!(
        !generated.contains("unsupported nested function"),
        "nested function should no longer emit a TPZ6PY decline: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested function Python gate failed: {e}"));
}

#[test]
fn emits_nested_function_nonlocal_assignment() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut counter = 0
    function bump() -> int {
        counter = counter + 1
        counter
    }
    bump()
}
main()
"#,
    );
    assert!(
        generated.contains("nonlocal _t_636f756e746572"),
        "outer counter write should declare a Python nonlocal: {generated}"
    );
    assert_generated_python_ok_int(&generated, 1, "nested function nonlocal assignment");
}

#[test]
fn emits_nested_function_nonlocal_assignment_inside_if() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut counter = 0
    function bump() -> int {
        let marker = if true {
            counter = 5
            1
        } else {
            0
        }
        marker
    }
    let got = bump()
    counter + got
}
main()
"#,
    );
    assert!(
        generated.contains("nonlocal _t_636f756e746572"),
        "outer counter write in branch should declare a Python nonlocal: {generated}"
    );
    assert_generated_python_ok_int(&generated, 6, "nested function if nonlocal assignment");
}

#[test]
fn emits_nested_function_local_assignment() {
    let generated = emit_source(
        r#"
function main() -> int {
    function bump() -> int {
        let mut x = 0
        x = x + 1
        x
    }
    bump()
}
main()
"#,
    );
    assert!(
        generated.contains("_t_78 = tpz_add"),
        "nested function should emit local assignment arithmetic: {generated}"
    );
    assert!(
        !generated.contains("unsupported nested function assignment"),
        "local nested assignment should not emit a TPZ6PY decline: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested function local assignment Python gate failed: {e}"));
}

#[test]
fn emits_nested_function_branch_local_assignment() {
    let generated = emit_source(
        r#"
function main() -> int {
    function choose() -> int {
        if true {
            let mut x = 1
            x = x + 1
            x
        } else {
            0
        }
    }
    choose()
}
main()
"#,
    );
    assert!(
        generated.contains("if tpz_condition(True"),
        "nested branch should lower to Python if: {generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("nested function branch-local assignment Python gate failed: {e}")
    });
}

#[test]
fn nested_function_parameter_reassignment_declines_as_immutable_assignment() {
    let error = emit_error_for_source(
        r#"
function main() -> int {
    function bump(x: int) -> int {
        x = x + 1
        x
    }
    bump(4)
}
main()
"#,
    );
    assert_eq!(error.code(), "TPZ6PY0001");
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => assert_eq!(what, "assign to immutable"),
        other => panic!("expected immutable parameter assignment decline, got {other:?}"),
    }
    assert!(
        error.span.is_some(),
        "nested parameter assignment decline should carry a source span"
    );
}

#[test]
fn emits_nested_function_local_compound_assignment() {
    let generated = emit_source(
        r#"
function main() -> int {
    function bump() -> int {
        let mut x = 1
        x += 2
        x
    }
    bump()
}
main()
"#,
    );
    assert!(
        generated.contains("_t_78 = __tpz_assign_next"),
        "nested function should emit local compound assignment: {generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("nested function local compound assignment Python gate failed: {e}")
    });
}

#[test]
fn emits_nested_function_inner_local_assignment() {
    let generated = emit_source(
        r#"
function main() -> int {
    function outer() -> int {
        function inner() -> int {
            let mut x = 1
            x = x + 1
            x
        }
        inner()
    }
    outer()
}
main()
"#,
    );
    assert!(
        generated.contains("def _t_696e6e6572()"),
        "inner nested function should emit as a local Python def: {generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("nested function inner-local assignment Python gate failed: {e}")
    });
}

#[test]
fn emits_nested_function_forward_reference_after_definitions() {
    let generated = emit_source(
        r#"
function main() -> int {
    function first() -> int {
        second()
    }
    function second() -> int {
        1
    }
    first()
}
main()
"#,
    );
    assert!(
        generated.contains("_t_6669727374()"),
        "first nested function should call through local callable path: {generated}"
    );
    assert!(
        generated.contains("_t_7365636f6e64()"),
        "forward sibling should be known while emitting the earlier body: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested function forward reference Python gate failed: {e}"));
}

#[test]
fn emits_nested_function_mutual_recursion() {
    let generated = emit_source(
        r#"
function main() -> int {
    function isEven(n: int) -> bool {
        if n == 0 {
            true
        } else {
            isOdd(n - 1)
        }
    }
    function isOdd(n: int) -> bool {
        if n == 0 {
            false
        } else {
            isEven(n - 1)
        }
    }
    if isOdd(5) { 1 } else { 0 }
}
main()
"#,
    );
    assert!(
        generated.contains("_t_69734576656e("),
        "mutual recursion should emit isEven local def: {generated}"
    );
    assert!(
        generated.contains("_t_69734f6464("),
        "mutual recursion should emit isOdd local def: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested function mutual recursion Python gate failed: {e}"));
}

#[test]
fn nested_function_call_before_definition_emits_positional_fault() {
    let generated = emit_source(
        r#"
function main() -> int {
    log()
    function log() -> int {
        1
    }
    0
}
main()
"#,
    );
    assert!(
        generated.contains("__tpz_forward_function(_t_6c6f67, \"log\","),
        "call-before-definition must read the missing-aware function cell: {generated}"
    );
    assert_generated_python_fault_code(&generated, "TPZ5002", "nested function forward read");
}

#[test]
fn nested_function_transitive_call_before_definition_emits_positional_fault() {
    let generated = emit_source(
        r#"
function main() -> int {
    function first() -> int {
        second()
    }
    first()
    function second() -> int {
        1
    }
    0
}
main()
"#,
    );
    assert_generated_python_fault_code(&generated, "TPZ5002", "nested transitive forward read");
}

#[test]
fn nested_function_multistep_transitive_call_before_definition_emits_positional_fault() {
    let generated = emit_source(
        r#"
function main() -> int {
    function caller() -> int {
        first()
    }
    function first() -> int {
        second()
    }
    caller()
    function second() -> int {
        1
    }
    0
}
main()
"#,
    );
    assert_generated_python_fault_code(&generated, "TPZ5002", "nested multistep forward read");
}

#[test]
fn nested_function_branch_local_transitive_call_before_definition_emits_positional_fault() {
    let generated = emit_source(
        r#"
function main() -> int {
    function first() -> int {
        second()
    }
    if true {
        first()
    }
    function second() -> int {
        1
    }
    0
}
main()
"#,
    );
    assert_generated_python_fault_code(&generated, "TPZ5002", "nested branch-local forward read");
}

#[test]
fn nested_function_dead_branch_transitive_forward_is_not_eagerly_rejected() {
    let generated = emit_source(
        r#"
function main() -> int {
    function first() -> int {
        if false {
            second()
        } else {
            3
        }
    }
    first()
    function second() -> int {
        1
    }
    0
}
main()
"#,
    );
    assert_generated_python_ok_int(&generated, 0, "nested dead-branch forward read");
}

#[test]
fn nested_function_iife_forward_reference_emits_positional_fault() {
    let generated = emit_source(
        r#"
function main() -> int {
    let value = (() => later())()
    function later() -> int {
        1
    }
    value
}
main()
"#,
    );
    assert_generated_python_fault_code(&generated, "TPZ5002", "nested IIFE forward read");
}

#[test]
fn nested_function_parenthesized_iife_forward_reference_emits_positional_fault() {
    let generated = emit_source(
        r#"
function main() -> int {
    let value = ((() => later()))()
    function later() -> int {
        1
    }
    value
}
main()
"#,
    );
    assert_generated_python_fault_code(
        &generated,
        "TPZ5002",
        "nested parenthesized IIFE forward read",
    );
}

#[test]
fn nested_function_iife_transitive_forward_reference_emits_positional_fault() {
    let generated = emit_source(
        r#"
function main() -> int {
    function first() -> int {
        second()
    }
    let value = (() => first())()
    function second() -> int {
        1
    }
    value
}
main()
"#,
    );
    assert_generated_python_fault_code(
        &generated,
        "TPZ5002",
        "nested IIFE transitive forward read",
    );
}

#[test]
fn nested_function_iife_shadowed_outer_function_reads_outer_before_declaration() {
    let generated = emit_source(
        r#"
function later() -> int {
    0
}
function main() -> int {
    let value = (() => later())()
    function later() -> int {
        1
    }
    value
}
main()
"#,
    );
    assert!(
        generated.contains("[tpz_host_callable(_t_6c61746572, host"),
        "the positional cell must inherit the visible outer callable: {generated}"
    );
    assert_generated_python_ok_int(&generated, 0, "nested shadowed-outer forward read");
}

#[test]
fn emits_iife_lambda_param_shadow_without_forward_decline() {
    let generated = emit_source(
        r#"
function main() -> int {
    let value = ((later: int) => later)(3)
    function later() -> int {
        1
    }
    value
}
main()
"#,
    );
    assert!(
        generated.contains("lambda _t_6c61746572: _t_6c61746572"),
        "IIFE lambda parameter should shadow the later nested function: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("IIFE lambda param shadow Python gate failed: {e}"));
}

#[test]
fn emits_iife_lambda_param_shadow_over_transitive_candidate_without_forward_decline() {
    let generated = emit_source(
        r#"
function main() -> int {
    function first() -> int {
        second()
    }
    let value = ((first: int) => first)(3)
    function second() -> int {
        1
    }
    value
}
main()
"#,
    );
    assert!(
        generated.contains("lambda _t_6669727374: _t_6669727374"),
        "IIFE lambda parameter should shadow the earlier transitive candidate: {generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("IIFE lambda transitive candidate param shadow Python gate failed: {e}")
    });
}

#[test]
fn emits_iife_returned_lambda_with_delayed_nested_forward_reference() {
    let generated = emit_source(
        r#"
function main() -> int {
    let g = (() => (() => later()))()
    function later() -> int {
        1
    }
    1
}
main()
"#,
    );
    assert!(
        generated.contains("lambda : _t_6c61746572()"),
        "returned lambda should keep its body delayed until after later is defined: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("IIFE returned delayed lambda Python gate failed: {e}"));
}

#[test]
fn nested_function_parameter_default_forward_declines_loudly() {
    let error = emit_error_for_source(
        r#"
function main() -> int {
    function first(x: int = second()) -> int {
        x
    }
    function second() -> int {
        1
    }
    first()
}
main()
"#,
    );
    assert_eq!(error.code(), "TPZ6PY0001");
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => assert_eq!(what, "function default shape"),
        other => panic!("expected function default shape decline, got {other:?}"),
    }
    assert!(
        error.span.is_some(),
        "nested function parameter default decline should carry a source span"
    );
}

#[test]
fn emits_nested_function_shadowing_outer_function_with_scoped_name() {
    let generated = emit_source(
        r#"
function helper() -> int {
    1
}
function main() -> int {
    function helper() -> int {
        2
    }
    helper()
}
main()
"#,
    );
    assert!(
        generated.contains("def _t_68656c706572__s"),
        "nested helper should use a scoped Python name when it shadows top-level helper: {generated}"
    );
    assert_generated_python_ok_int(&generated, 2, "nested function shadowing outer function");
}

#[test]
fn emits_nested_function_assignment_sibling_reference() {
    let generated = emit_source(
        r#"
function main() -> int {
    function first() -> int {
        second()
    }
    function second() -> int {
        let mut x = 0
        x = x + 1
        x
    }
    first()
}
main()
"#,
    );
    assert!(
        generated.contains("_t_6669727374()"),
        "first nested function should call through local callable path: {generated}"
    );
    assert!(
        generated.contains("_t_78 = tpz_add(_t_78, 1"),
        "second nested function should keep its local assignment: {generated}"
    );
    assert_generated_python_gates(&generated).unwrap_or_else(|e| {
        panic!("nested function assignment sibling reference Python gate failed: {e}")
    });
}

#[test]
fn emits_nested_function_assign_before_local_shadow() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut seed = 1
    function bad() -> int {
        seed = 2
        let mut seed = 0
        seed
    }
    let got = bad()
    seed + got
}
main()
"#,
    );
    assert!(
        generated.contains("nonlocal _t_73656564") && generated.contains("_t_73656564__s"),
        "assign-before-local shadow should mutate the outer seed before creating a scoped local seed: {generated}"
    );
    assert_generated_python_ok_int(&generated, 2, "nested assign before local shadow");
}

#[test]
fn emits_nested_function_block_exited_local_shadow_outer_write() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut seed = 1
    function bad() -> int {
        if true {
            let mut seed = 0
            seed = seed + 1
        }
        seed = seed + 1
        seed
    }
    let got = bad()
    seed + got
}
main()
"#,
    );
    assert!(
        generated.contains("nonlocal _t_73656564") && generated.contains("_t_73656564__s"),
        "block-exited local shadow should keep a scoped local seed and mutate the outer seed afterward: {generated}"
    );
    assert_generated_python_ok_int(&generated, 4, "nested block-exited local shadow");
}

#[test]
fn nested_function_intra_function_block_shadow_assignment_emits_scoped_names() {
    let generated = emit_source(
        r#"
function main() -> int {
    function f() -> int {
        let mut x = 1
        if true {
            let mut x = 100
            x = x + 1
        } else {
        }
        x = x + 1
        x
    }
    f()
}
main()
"#,
    );
    assert!(
        generated.contains("_t_78__s"),
        "inner block-shadowed x must lower to a distinct Python local: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("block-shadow assignment Python gate failed: {e}"));
}

#[test]
fn nested_function_block_shadow_read_with_local_assignment_emits_scoped_names() {
    let generated = emit_source(
        r#"
function main() -> int {
    function f() -> int {
        let mut a = 1
        a = a + 1
        let x = 1
        if true {
            let x = 100
        } else {
        }
        a + x
    }
    f()
}
main()
"#,
    );
    assert!(
        generated.contains("_t_78__s"),
        "read-only inner block-shadowed x must lower to a distinct Python local: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("block-shadow read Python gate failed: {e}"));
}

#[test]
fn emits_nested_function_nonlocal_assignment_inside_while() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut counter = 0
    function bump() -> int {
        while counter < 1 {
            counter = counter + 1
        }
        counter
    }
    bump()
}
main()
"#,
    );
    assert!(
        generated.contains("nonlocal _t_636f756e746572"),
        "outer counter write in while should declare a Python nonlocal: {generated}"
    );
    assert_generated_python_ok_int(&generated, 1, "nested function while nonlocal assignment");
}

#[test]
fn emits_nested_function_inner_writes_middle_local() {
    let generated = emit_source(
        r#"
function main() -> int {
    function outer() -> int {
        let mut value = 0
        function inner() -> int {
            value = value + 1
            value
        }
        inner()
    }
    outer()
}
main()
"#,
    );
    assert!(
        generated.contains("nonlocal _t_76616c7565"),
        "inner should declare the middle function local value as nonlocal: {generated}"
    );
    assert_generated_python_ok_int(&generated, 1, "nested inner writes middle local");
}

#[test]
fn emits_nested_function_local_shadow_of_enclosing_binding() {
    let generated = emit_source(
        r#"
function main() -> int {
    let base = 10
    function bad() -> int {
        let mut base = 0
        base = base + 1
        base
    }
    bad()
}
main()
"#,
    );
    assert!(
        generated.contains("_t_62617365__s"),
        "local base shadow should use a scoped Python name: {generated}"
    );
    assert_generated_python_ok_int(&generated, 1, "nested local shadow of enclosing binding");
}

#[test]
fn value_collecting_for_emits_ordered_arrays_fresh_captures_and_outer_writes() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut total = 0
    let values = for [a, b] in [[1, 2], [3, 4]] {
        total = total + a + b
        total
    }
    let callbacks = for x in [5, 6] {
        () => x
    }
    let nested = for x in [1, 2] {
        for y in [10, 20] {
            x + y
        }
    }
    total * 100000 + values[0] * 10000 + values[1] * 1000 + callbacks[0]() * 100 + callbacks[1]() * 10 + nested[0][0] + nested[1][1]
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_for_pattern(")
            && generated.contains("def __tpz_for_body_")
            && generated.contains(".append(__tpz_for_body_"),
        "collecting for must use checked patterns and per-iteration body helpers: {generated}"
    );
    assert_generated_python_ok_int(&generated, 1_040_593, "value-collecting for");
}

#[test]
fn value_collecting_for_unit_body_and_pattern_mismatch_are_runtime_exact() {
    let unit_generated = emit_source(
        r#"
function main() -> int {
    let values = for x in [1, 2, 3] { () }
    values.length
}
main()
"#,
    );
    assert_generated_python_ok_int(&unit_generated, 3, "unit value-collecting for");

    let mismatch_generated = emit_source(
        r#"
function main() -> Array<int> {
    for [a, b] in [[1, 2], [3]] {
        a + b
    }
}
main()
"#,
    );
    assert_generated_python_fault_code(
        &mismatch_generated,
        "TPZ5001",
        "value-collecting for pattern mismatch",
    );
}

#[test]
fn statement_for_supports_general_patterns_and_faults_on_mismatch() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut total = 0
    for [a, b] in [[1, 2], [3, 4]] {
        total += a * 10 + b
    }
    total
}
main()
"#,
    );
    assert_generated_python_ok_int(&generated, 46, "statement for destructuring pattern");

    let mismatch = emit_source(
        r#"
function main() -> int {
    let mut total = 0
    for [a, b] in [[1, 2], [3]] {
        total += a + b
    }
    total
}
main()
"#,
    );
    assert_generated_python_fault_code(
        &mismatch,
        "TPZ5001",
        "statement for destructuring pattern mismatch",
    );
}

#[test]
fn value_collecting_for_preserves_iter_once_defer_return_and_outer_control() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut calls = 0
    let mut drains = 0
    function source() -> Array<int> {
        calls = calls + 1
        [1, 2]
    }
    function mark() {
        drains = drains + 1
    }
    function returnFromBody() -> int {
        let ignored = for x in [1, 2, 3] {
            if x == 2 { return 9 }
            x
        }
        0
    }
    let values = for x in source() {
        defer mark()
        drains * 10 + x
    }
    let escaped = loop 'outer {
        let ignored = for x in [1] {
            if x == 1 { break 'outer 7 }
            0
        }
        break 0
    }
    calls * 100000 + drains * 10000 + values[0] * 1000 + values[1] * 100 + returnFromBody() * 10 + escaped
}
main()
"#,
    );
    assert!(
        generated.contains("__tpz_run_defers_to(")
            && generated.contains("raise TpzReturn(")
            && generated.contains("raise TpzLoopBreak("),
        "collecting for must preserve defer and crossing control propagation: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        122_297,
        "value-collecting for effects and control",
    );
}

#[test]
fn value_collecting_for_uses_cooperative_helpers_inside_concurrent_arms() {
    let generated = emit_source(
        r#"
function main() -> int {
    let result = concurrent {
        collect: {
            let values = for x in [1, 2] {
                let mut i = 0
                while i < 2 { i = i + 1 }
                x
            }
            values.length
        }
        idle: 0
    }
    result.collect
}
main()
"#,
    );
    assert!(
        generated.contains("yield from __tpz_for_body_")
            && generated.contains("def __tpz_for_body_")
            && generated.contains("if False:"),
        "collecting for helpers inside cooperative arms must remain generators: {generated}"
    );
    assert_generated_python_ok_int(&generated, 2, "cooperative value-collecting for");
}

#[test]
fn cooperative_for_families_yield_before_each_iteration_body() {
    let cases = [
        (
            "statement for cooperative iteration",
            r#"
function main() -> int {
    let mut trace: Array<int> = []
    concurrent {
        iterate: {
            for x in [1, 2] { trace.push(x) }
            0
        }
        observe: {
            trace.push(9)
            0
        }
    }
    trace[0] * 100 + trace[1] * 10 + trace[2]
}
main()
"#,
        ),
        (
            "value for cooperative iteration",
            r#"
function main() -> int {
    let mut trace: Array<int> = []
    concurrent {
        iterate: {
            let values = for x in [1, 2] {
                trace.push(x)
                x
            }
            values.length
        }
        observe: {
            trace.push(9)
            0
        }
    }
    trace[0] * 100 + trace[1] * 10 + trace[2]
}
main()
"#,
        ),
        (
            "comprehension cooperative iteration",
            r#"
function main() -> int {
    let mut trace: Array<int> = []
    concurrent {
        iterate: {
            let values = [ for x in [1, 2] => trace.push(x) ]
            values.length
        }
        observe: {
            trace.push(9)
            0
        }
    }
    trace[0] * 100 + trace[1] * 10 + trace[2]
}
main()
"#,
        ),
    ];

    for (name, source) in cases {
        let generated = emit_source(source);
        assert_generated_python_ok_int(&generated, 912, name);
    }
}

#[test]
fn value_collecting_for_lowers_through_expression_containers() {
    let generated = emit_source(
        r#"
function count(xs: Array<int>) -> int { xs.length }
function main() -> int {
    let argument = count(for x in [1, 2, 3] { x })
    let wrapped = [(for x in [4] { x + 1 })]
    let record = { values: for x in [6] { x + 1 } }
    let branch = if true { for x in [8] { x + 1 } } else { [] }
    let matched = match 1 {
        case 1 => for x in [10] { x + 1 }
        case _ => []
    }
    let piped = (for x in [12, 13] { x }) |> .length
    let comp = [ for seed in [1, 2] => for x in [20] { seed + x } ]
    argument * 100000 + wrapped[0][0] * 10000 + record.values[0] * 1000 + branch[0] * 100 + matched[0] * 10 + piped + comp[0][0] + comp[1][0]
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        358_055,
        "value-collecting for expression containers",
    );
}

#[test]
fn value_collecting_for_lowers_inside_lambda_bodies() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut total = 0
    let collect = (base: int) => for x in [1, 2] {
        total = total + base + x
        total
    }
    let values = collect(3)
    values[0] * 1000 + values[1] * 100 + total
}
main()
"#,
    );
    assert!(
        generated.contains("def __tpz_lambda_body_")
            && generated.contains("nonlocal _t_746f74616c")
            && generated.contains("def __tpz_for_body_"),
        "collecting for inside a lambda must use nested function boundaries: {generated}"
    );
    assert_generated_python_ok_int(&generated, 4_909, "value-collecting for lambda body");
}
