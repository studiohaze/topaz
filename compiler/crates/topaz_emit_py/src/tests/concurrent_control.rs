use super::*;

#[test]
fn emits_loop_expression_in_lazy_binary_positions() {
    let generated = emit_source(
        r#"
function main() -> int {
    let andSkipped = false && loop {
        print("and-skip")
        break true
    }
    let andHit = true && loop {
        print("and-hit")
        break true
    }
    let orSkipped = true || loop {
        print("or-skip")
        break false
    }
    let orHit = false || loop {
        print("or-hit")
        break true
    }
    let some: Option<int> = Some(7)
    let none: Option<int> = None
    let coalesceSkipped = some ?? loop {
        print("coalesce-skip")
        break 9
    }
    let coalesceHit = none ?? loop {
        print("coalesce-hit")
        break 11
    }
    print("{andSkipped}:{andHit}:{orSkipped}:{orHit}:{coalesceSkipped}:{coalesceHit}")
    let result = if andHit && orHit {
        coalesceHit
    } else {
        0
    }
    result
}
main()
"#,
    );
    assert!(
        generated.contains("if tpz_condition(") && generated.contains("else:"),
        "lazy boolean nested loop positions should lower into explicit branches: {generated}"
    );
    assert!(
        generated.contains("if isinstance(")
            && generated.contains("elif ")
            && generated.contains(" is None or ")
            && generated.contains(" is TPZ_NULL:"),
        "coalesce nested loop positions should preserve Some/None/null branching: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, True)")
            && generated.contains("raise TpzLoopBreak(None, False)")
            && generated.contains("raise TpzLoopBreak(None, 11)"),
        "lazy nested loop break values should stay inside lowered branches: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested loop lazy binary Python gate failed: {e}"));
}

#[test]
fn emits_loop_expression_in_if_and_match_result_positions() {
    let generated = emit_source(
        r#"
function main() -> int {
    let choose = true
    let fromIf = if choose {
        loop {
            print("if-then")
            break 3
        }
    } else {
        loop {
            print("if-else")
            break 4
        }
    }
    let n = 2
    let fromMatch = match n {
        case 1 => loop {
            print("one")
            break 10
        }
        case 2 => loop {
            print("two")
            break fromIf + 20
        }
        case _ => 0
    }
    print("{fromIf}:{fromMatch}")
    fromIf + fromMatch
}
main()
"#,
    );
    assert!(
        generated.contains("if tpz_condition(") && generated.contains("elif "),
        "if/match nested loop positions should lower into explicit branches: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, 3)")
            && generated.contains("raise TpzLoopBreak(None, 4)")
            && generated.contains("raise TpzLoopBreak(None, tpz_add("),
        "if/match branch loop values should lower through loop-control values: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested loop if/match result position Python gate failed: {e}"));
}

#[test]
fn emits_loop_expression_in_match_guards() {
    let generated = emit_source(
        r#"
function main() -> int {
    let x = 2
    let result = match x {
        case n if loop {
            print("guard-one")
            break n == 1
        } => 10
        case n if loop {
            print("guard-two")
            break n == 2
        } => loop {
            print("body-two")
            break 20
        }
        case _ => 0
    }
    print("{result}")
    result
}
main()
"#,
    );
    assert!(
        generated.contains("match_done") && generated.contains("if not"),
        "match guard nested loop positions should lower through a matched flag: {generated}"
    );
    assert!(
        generated.contains("tpz_condition("),
        "lowered match guards should still use Topaz truthiness: {generated}"
    );
    assert!(
        generated.contains("raise TpzLoopBreak(None, tpz_eq(")
            && generated.contains("raise TpzLoopBreak(None, 20)"),
        "match guard and body loops should lower through loop-control values: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("nested loop match guard Python gate failed: {e}"));
}

#[test]
fn cooperative_match_guards_use_context_aware_statement_lowering() {
    let cases = [
        (
            "statement match cooperative guard",
            r#"
function main() -> int {
    let mut trace: Array<int> = []
    concurrent {
        evaluate: {
            match 1 {
                case 1 if [ for x in [1, 2] => trace.push(x) ].length == 2 => trace.push(3)
                case _ => ()
            }
            0
        }
        observe: {
            trace.push(9)
            0
        }
    }
    trace[0] * 1000 + trace[1] * 100 + trace[2] * 10 + trace[3]
}
main()
"#,
        ),
        (
            "value match cooperative guard",
            r#"
function main() -> int {
    let mut trace: Array<int> = []
    concurrent {
        evaluate: {
            let selected = match 1 {
                case 1 if [ for x in [1, 2] => trace.push(x) ].length == 2 => 3
                case _ => 0
            }
            trace.push(selected)
            0
        }
        observe: {
            trace.push(9)
            0
        }
    }
    trace[0] * 1000 + trace[1] * 100 + trace[2] * 10 + trace[3]
}
main()
"#,
        ),
    ];

    for (name, source) in cases {
        let generated = emit_source(source);
        assert_generated_python_ok_int(&generated, 9_123, name);
    }
}
