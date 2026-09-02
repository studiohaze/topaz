//! End-to-end machine tests: parse real Topaz source, run it, and
//! pin the §2/§5/§12 interpreter semantics.

use topaz_diag::FileId;
use topaz_interp::machine::codes;
use topaz_interp::{Machine, Value, render};
use topaz_parser::{ParseOptions, parse, parse_with_options};
use topaz_syntax::LangVersion;

/// The interpreter targets v5.2 (CDR-003); parse accordingly.
fn parse_v52(src: &str) -> topaz_parser::ParseOutput {
    parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_2,
        },
    )
}

fn run(src: &str) -> Result<String, (String, String)> {
    run_with_host(src, &topaz_interp::TestHost::new()).map(|(v, _)| v)
}

fn run_with_host(
    src: &str,
    host: &topaz_interp::TestHost,
) -> Result<(String, Vec<String>), (String, String)> {
    let out = parse_v52(src);
    assert!(
        out.diagnostics.is_empty(),
        "test source must parse: {:?}",
        out.diagnostics
    );
    let program = out.program;
    let mut machine = Machine::new(src, host);
    match machine.run_program(&program) {
        Ok(v) => Ok((render(&v), host.stdout())),
        Err(e) => Err((e.code.to_string(), e.message)),
    }
}

fn run_value(src: &str) -> String {
    run(src).expect("expected successful run")
}

fn run_error(src: &str) -> (String, String) {
    run(src).expect_err("expected a runtime stop")
}

#[test]
fn arithmetic_and_precedence() {
    assert_eq!(run_value("1 + 2 * 3"), "7");
    assert_eq!(run_value("2 ** 3 ** 2"), "512"); // right-assoc
    assert_eq!(run_value("7 / 2"), "3"); // truncation toward zero
    assert_eq!(run_value("-7 % 3"), "-1"); // sign of a
    assert_eq!(run_value("1.5 + 1.5"), "3.0");
    assert_eq!(run_value("\"a\" + \"b\""), "ab");
}

#[test]
fn faults_carry_their_codes() {
    assert_eq!(run_error("1 / 0").0, codes::FAULT_DIV_ZERO);
    assert_eq!(
        run_error("9223372036854775807 + 1").0,
        codes::FAULT_OVERFLOW
    );
    assert_eq!(run_error("let x = [1, 2]\nx[5]").0, codes::FAULT_INDEX);
    assert_eq!(run_error("let n = -1\n2 ** n").0, codes::FAULT_NEG_EXPONENT);
}

#[test]
fn guards_are_tpz5() {
    assert_eq!(run_error("nope").0, codes::GUARD_UNBOUND);
    assert_eq!(run_error("1 + \"a\"").0, codes::GUARD_TYPE);
    assert_eq!(run_error("let x = 1\nx = 2\nx").0, codes::GUARD_IMMUTABLE);
    assert_eq!(run_error("1.0 % 2.0").0, codes::GUARD_TYPE);
}

#[test]
fn bindings_scopes_and_mutation() {
    assert_eq!(run_value("let x = 1\nlet mut y = 2\ny += x\ny"), "3");
    assert_eq!(
        run_value("let x = 1\nlet y = { let x = 10\n x + 1 }\nx + y"),
        "12"
    );
}

#[test]
fn if_while_break_continue() {
    assert_eq!(run_value("if 1 < 2 { \"yes\" } else { \"no\" }"), "yes");
    assert_eq!(
        run_value(
            "let mut sum = 0\nlet mut i = 0\nwhile true {\n    i += 1\n    if i > 10 { break }\n    if i % 2 == 0 { continue }\n    sum += i\n}\nsum"
        ),
        "25"
    );
}

#[test]
fn functions_closures_and_pipes() {
    assert_eq!(
        run_value("function add(a: int, b: int) -> int { return a + b }\nadd(2, 3)"),
        "5"
    );
    assert_eq!(
        run_value("let make = (n: int) => (m: int) => n + m\nlet add3 = make(3)\nadd3(4)"),
        "7"
    );
    assert_eq!(
        run_value("let double = (x: int) => x * 2\n5 |> double"),
        "10"
    );
    assert_eq!(run_value("let r = { value: 42 }\nr |> .value"), "42");
}

#[test]
fn deep_recursion_faults_at_the_shared_limit() {
    // The interpreter recurses on an explicit HEAP frame stack (never the Rust stack),
    // so recursion WITHIN the shared `CALL_DEPTH_LIMIT` runs cleanly...
    assert_eq!(
        run_value(
            "function down(n: int) -> int {\n    if n == 0 { return 0 }\n    return down(n - 1)\n}\ndown(900)"
        ),
        "0"
    );
    // ...and deeper recursion now faults at that SHARED cap with a clean TPZ5009 —
    // matching the emitted native binary (which would otherwise overflow its stack)
    // instead of diverging (§4 recursion-limit parity).
    let (code, _) = run_error(
        "function down(n: int) -> int {\n    if n == 0 { return 0 }\n    return down(n - 1)\n}\ndown(200000)",
    );
    assert_eq!(code, codes::GUARD_RECURSION);
}

#[test]
fn records_are_values() {
    assert_eq!(
        run_value("let a = { x: 1, y: 2 }\nlet b = a { x: 10 }\na.x + b.x + b.y"),
        "13"
    );
    let (code, _) = run_error("let a = { x: 1 }\nlet b = a { z: 9 }\nb");
    assert_eq!(code, codes::GUARD_NO_FIELD);
}

#[test]
fn arrays_share_and_interpolation_renders() {
    assert_eq!(run_value("let xs = [1, 2, 3]\nxs.length"), "3");
    assert_eq!(
        run_value("let xs = [1, 2]\nlet ys = [0, ...xs]\nys"),
        "[0, 1, 2]"
    );
    assert_eq!(
        run_value("let name = \"topaz\"\n\"hi {name}{1 + 1}\""),
        "hi topaz2"
    );
    assert_eq!(run_value("\"line\\nnext\""), "line\nnext");
}

#[test]
fn option_null_coalescing_and_optional_access() {
    assert_eq!(run_value("let x = Some(5)\nx ?? 0"), "5"); // unwraps one layer
    assert_eq!(run_value("let x = None\nx ?? 7"), "7");
    assert_eq!(run_value("null ?? 3"), "3");
    assert_eq!(run_value("let r = Some({ v: 1 })\nr?.v"), "Some(1)");
    assert_eq!(run_value("let r = None\nr?.v"), "None");
    assert_eq!(run_value("let mut x = None\nx ??= Some(2)\nx ?? 0"), "2");
}

#[test]
fn membership() {
    assert_eq!(run_value("2 in [1, 2, 3]"), "true");
    assert_eq!(run_value("\"z\" in [\"a\"]"), "false");
}

#[test]
fn machine_returns_last_expression_value() {
    let out = parse(FileId(0), "let x = 5\nx * 2");
    assert!(out.diagnostics.is_empty());
    let th = topaz_interp::TestHost::new();
    let mut m = Machine::new("let x = 5\nx * 2", &th);
    let program = parse(FileId(0), "let x = 5\nx * 2").program;
    let v = m.run_program(&program).expect("runs");
    assert!(matches!(v, Value::Int(10)));
}

#[test]
fn match_patterns_guards_and_miss() {
    assert_eq!(
        run_value(
            "let x = 5\nmatch x {\n    case 0 => \"zero\"\n    case n if n > 3 => \"big\"\n    case _ => \"small\"\n}"
        ),
        "big"
    );
    assert_eq!(
        run_value("match Some(3) {\n    case Some(v) => v * 10\n    case None => 0\n}"),
        "30"
    );
    assert_eq!(
        run_value("match \"b\" {\n    case \"a\" | \"b\" => 1\n    case _ => 2\n}"),
        "1"
    );
    assert_eq!(
        run_value("match 7 {\n    case 1..10 => \"in\"\n    case _ => \"out\"\n}"),
        "in"
    );
    assert_eq!(
        run_value("match 1 {\n    case x: string => \"s\"\n    case x: int => \"i\"\n}"),
        "i"
    );
    assert_eq!(
        run_error("match 9 {\n    case 1 => 1\n}").0,
        codes::FAULT_MATCH_MISS
    );
}

#[test]
fn destructuring_let_and_list_patterns() {
    assert_eq!(run_value("let { x, y } = { x: 1, y: 2 }\nx + y"), "3");
    assert_eq!(
        run_value("let [first, ..rest] = [1, 2, 3]\nfirst + rest.length"),
        "3"
    );
    assert_eq!(
        run_value("match [1, 2, 3] {\n    case [1, ..mid] => mid\n    case _ => []\n}"),
        "[2, 3]"
    );
}

#[test]
fn try_propagation() {
    assert_eq!(
        run_value(
            "function f(r: Result<int, string>) -> Result<int, string> {\n    let v = r?\n    return Ok(v + 1)\n}\nf(Ok(1)) ?? 0"
        ),
        "Ok(2)"
    );
    assert_eq!(
        run_value(
            "function f(r: Result<int, string>) -> Result<int, string> {\n    let v = r?\n    return Ok(v + 1)\n}\nmatch f(Err(\"bad\")) {\n    case Err(e) => e\n    case Ok(v) => \"ok\"\n}"
        ),
        "bad"
    );
}

#[test]
fn for_loops_and_ranges() {
    assert_eq!(
        run_value("let mut sum = 0\nfor x in [1, 2, 3] { sum += x }\nsum"),
        "6"
    );
    assert_eq!(
        run_value("let mut sum = 0\nfor i in 1..4 { sum += i }\nsum"),
        "10"
    );
    assert_eq!(
        run_value("let mut sum = 0\nfor i in 0..<10 by 3 { sum += i }\nsum"),
        "18"
    );
    assert_eq!(
        run_value("let squares = for x in 1..3 { x * x }\nsquares"),
        "[1, 4, 9]"
    );
    assert_eq!(run_value("4 in 0..10 by 2"), "true");
    assert_eq!(run_value("5 in 0..10 by 2"), "false");
    assert_eq!(
        run_error("let mut s = 0\nlet z = s\nfor i in 0..3 by z { s += i }\ns").0,
        codes::FAULT_RANGE_STEP
    );
}

#[test]
fn compose() {
    assert_eq!(
        run_value(
            "let inc = (x: int) => x + 1\nlet dbl = (x: int) => x * 2\nlet f = inc >> dbl\nf(5)"
        ),
        "12"
    );
}

#[test]
fn unwinding_keeps_the_value_stack_balanced() {
    // Reviewer counterexamples: partial operands must not leak
    // across return/break unwinds.
    assert_eq!(
        run_value(
            "function f() -> int {\n    while true {\n        let y = 1 + { return 7 }\n    }\n    return 0\n}\n10 + f()"
        ),
        "17"
    );
    assert_eq!(
        run_value(
            "function g() -> int {\n    while true { [1 + { break }] }\n    return 7\n}\n10 + g()"
        ),
        "17"
    );
}

#[test]
fn coalesce_assign_is_lazy() {
    // §12: present target -> RHS never evaluated, no write.
    assert_eq!(
        run_value("let mut x = Some(1)\nx ??= Some(1 / 0)\nx ?? 0"),
        "1"
    );
    assert_eq!(
        run_error("let x = None\nx ??= Some(1)\nx").0,
        codes::GUARD_IMMUTABLE
    );
}

#[test]
fn prelude_shadowing_is_uniform() {
    assert_eq!(
        run_value("function Some(x: int) -> int { return x * 2 }\nSome(3)"),
        "6"
    );
}

#[test]
fn prelude_constructors_accept_named_value_argument() {
    assert_eq!(run_value("Some(value: 1)"), "Some(1)");
    assert_eq!(run_value("Ok(value: 2)"), "Ok(2)");
    assert_eq!(run_value("Err(value: 3)"), "Err(3)");
}

#[test]
fn bare_none_is_a_constructor_pattern_not_a_binding() {
    // §6/§22.1: `None` is the polymorphic Option constructor, never
    // an ordinary variable — in pattern position it matches only the
    // None value. A `case None` arm before `case Some` must not
    // catch Some.
    assert_eq!(
        run_value("match toInt(\"3\") {\n    case None => 0\n    case Some(v) => v\n}"),
        "3"
    );
    assert_eq!(
        run_value("match toInt(\"x\") {\n    case None => 0\n    case Some(v) => v\n}"),
        "0"
    );
}

#[test]
fn same_scope_redeclaration_guards() {
    assert_eq!(
        run_error("let x = 1\nlet x = 2\nx").0,
        codes::GUARD_REDECLARE
    );
    assert_eq!(
        run_error("function f() -> int { return 1 }\nlet f = 2\nf").0,
        codes::GUARD_REDECLARE
    );
    // Nested scopes still shadow legally.
    assert_eq!(run_value("let x = 1\n{ let x = 2\n x }"), "2");
}

#[test]
fn prelude_print_and_strings() {
    let host = topaz_interp::TestHost::new();
    let (_, out) = run_with_host(
        "let name = \"topaz\"\nprint(\"hello {name}\")\nprint(\"bye\")",
        &host,
    )
    .expect("runs");
    assert_eq!(out, vec!["hello topaz", "bye"]);
    assert_eq!(run_error("print(1)").0, codes::GUARD_TYPE);
    assert_eq!(run_value("\"topaz\".scalars().length"), "5");
    assert_eq!(run_error("\"abc\".length").0, codes::GUARD_TYPE);
    assert_eq!(run_value("toInt(\"42\") ?? 0"), "42");
    assert_eq!(run_value("toInt(\"x\") ?? -1"), "-1");
}

#[test]
fn prelude_collections() {
    assert_eq!(
        run_value("let mut xs = Array.of(1, 2)\nxs.push(3)\nxs"),
        "[1, 2, 3]"
    );
    assert_eq!(run_value("let xs = [10, 20]\nxs.get(1) ?? 0"), "20");
    assert_eq!(run_value("let xs = [10]\nxs.get(5) ?? -1"), "-1");
    assert_eq!(
        run_value(
            "let mut m = Map.new()\nm.insert(\"a\", 1)\nm.insert(\"b\", 2)\nm.insert(\"a\", 9)\nm.keys"
        ),
        "[a, b]"
    );
    assert_eq!(
        run_value("let mut m = Map.new()\nm.insert(\"a\", 1)\nm.remove(\"a\") ?? 0"),
        "1"
    );
    assert_eq!(
        run_value("let mut s = Set.of(1, 2)\ns.add(2)\ns.remove(9)"),
        "false"
    );
    assert_eq!(run_value("let s = Set.of(1)\n2 in s"), "false");
}

#[test]
fn prelude_hofs() {
    assert_eq!(run_value("map([1, 2, 3], (x: int) => x * 2)"), "[2, 4, 6]");
    // `..` is inclusive (§10): 1..6 yields 1 through 6.
    assert_eq!(
        run_value("filter(1..6, (x: int) => x % 2 == 0)"),
        "[2, 4, 6]"
    );
    assert_eq!(run_value("filter(1..<6, (x: int) => x % 2 == 0)"), "[2, 4]");
    assert_eq!(
        run_value("reduce([1, 2, 3], 10, (acc: int, x: int) => acc + x)"),
        "16"
    );
}

#[test]
fn prelude_files_via_host() {
    let host = topaz_interp::TestHost::new();
    host.add_file("data.txt", "payload");
    let (v, _) = run_with_host(
        "let r = open(\"data.txt\")\nmatch r {\n    case Ok(f) => {\n        let text = match f.read() {\n            case Ok(tx) => tx\n            case Err(e) => e\n        }\n        f.write(\"new\")\n        f.close()\n        text\n    }\n    case Err(e) => e\n}",
        &host,
    )
    .expect("runs");
    assert_eq!(v, "payload");
    assert_eq!(
        host.files().get("data.txt").map(String::as_str),
        Some("new")
    );
    assert_eq!(
        run_value("match open(\"missing\") {\n    case Ok(f) => \"ok\"\n    case Err(e) => e\n}"),
        "cannot open `missing`: not found"
    );
}

#[test]
fn defer_runs_lifo_on_scope_exit() {
    let host = topaz_interp::TestHost::new();
    run_with_host(
        "{\n    defer print(\"first-registered\")\n    defer print(\"last-registered\")\n    print(\"body\")\n}",
        &host,
    )
    .expect("runs");
    assert_eq!(
        host.stdout(),
        vec!["body", "last-registered", "first-registered"]
    );
}

#[test]
fn defer_runs_on_return_and_try_unwinds() {
    let host = topaz_interp::TestHost::new();
    run_with_host(
        "function f() -> int {\n    defer print(\"cleanup\")\n    return 1\n}\nprint(\"{f()}\")",
        &host,
    )
    .expect("runs");
    assert_eq!(host.stdout(), vec!["cleanup", "1"]);

    let host2 = topaz_interp::TestHost::new();
    run_with_host(
        "function g(r: Result<int, string>) -> Result<int, string> {\n    defer print(\"closed\")\n    let v = r?\n    return Ok(v)\n}\nmatch g(Err(\"x\")) {\n    case Err(e) => print(\"err {e}\")\n    case Ok(v) => ()\n}",
        &host2,
    )
    .expect("runs");
    assert_eq!(host2.stdout(), vec!["closed", "err x"]);
}

#[test]
fn defer_runs_on_break_and_at_program_end() {
    let host = topaz_interp::TestHost::new();
    run_with_host(
        "let mut i = 0\nwhile true {\n    defer print(\"iter {i}\")\n    i += 1\n    if i == 2 { break }\n}",
        &host,
    )
    .expect("runs");
    // Deferred actions evaluate at scope exit (§14 registers the
    // action, not a snapshot), so each iteration prints the
    // incremented value.
    assert_eq!(host.stdout(), vec!["iter 1", "iter 2"]);

    let host2 = topaz_interp::TestHost::new();
    run_with_host("defer print(\"end\")\nprint(\"main\")", &host2).expect("runs");
    assert_eq!(host2.stdout(), vec!["main", "end"]);
}

#[test]
fn defer_errors_go_to_the_policy_hook() {
    let host = topaz_interp::TestHost::new();
    run_with_host(
        "function fail() -> Result<int, string> { return Err(\"boom\") }\n{\n    defer fail()\n    print(\"body\")\n}",
        &host,
    )
    .expect("runs — the Err never replaces the result");
    assert_eq!(host.stdout(), vec!["body"]);
    assert_eq!(host.defer_errors(), vec!["boom"]);
}

#[test]
fn defer_does_not_replace_inflight_err() {
    let host = topaz_interp::TestHost::new();
    let (v, _) = run_with_host(
        "function f() -> Result<int, string> {\n    defer print(\"d\")\n    let x = Err(\"original\")?\n    return Ok(x)\n}\nmatch f() {\n    case Err(e) => e\n    case Ok(v) => \"ok\"\n}",
        &host,
    )
    .expect("runs");
    assert_eq!(v, "original");
    assert_eq!(host.stdout(), vec!["d"]);
}

#[test]
fn concurrent_join_returns_arm_record() {
    assert_eq!(
        run_value("let r = concurrent {\n    a: 1 + 1\n    b: \"x\" + \"y\"\n}\n\"{r.a} {r.b}\""),
        "2 xy"
    );
}

#[test]
fn concurrent_err_arm_is_a_value_not_a_failure() {
    assert_eq!(
        run_value(
            "let r = concurrent {\n    good: Ok(1)\n    bad: Err(\"e\")\n}\nmatch r.bad {\n    case Err(e) => e\n    case Ok(v) => \"ok\"\n}"
        ),
        "e"
    );
}

#[test]
fn concurrent_arm_fault_before_timeout_faults_the_whole() {
    assert_eq!(
        run_error("concurrent {\n    boom: 1 / 0\n    fine: 2\n}").0,
        codes::FAULT_DIV_ZERO
    );
    // Fault in a later arm surfaces even when an earlier arm spins:
    // the CDR-003 antithesis counterexample.
    let host = topaz_interp::TestHost::new();
    let out = run_with_host(
        "function spin(n: int) -> int {\n    let mut i = 0\n    while i < n { i += 1 }\n    return i\n}\nconcurrent {\n    slow: spin(100000)\n    boom: 1 / 0\n}",
        &host,
    );
    assert_eq!(out.expect_err("must fault").0, codes::FAULT_DIV_ZERO);
}

#[test]
fn concurrent_timeout_runs_else_and_abandons() {
    let host = topaz_interp::TestHost::new();
    host.set_tick_per_poll(10); // each round advances the clock
    let (v, _) = run_with_host(
        "function spin() -> int {\n    let mut i = 0\n    while true { i += 1 }\n    return i\n}\nlet r = concurrent(timeout: 5ms) {\n    stuck: spin()\n} else { { stuck: -1 } }\nr.stuck",
        &host,
    )
    .expect("else path runs");
    assert_eq!(v, "-1");
}

#[test]
fn concurrent_completes_before_timeout() {
    let host = topaz_interp::TestHost::new();
    // Frozen clock: the deadline never expires; arms finish.
    let (v, _) = run_with_host(
        "let r = concurrent(timeout: 1ms) {\n    quick: 21 * 2\n} else { { quick: 0 } }\nr.quick",
        &host,
    )
    .expect("arms complete");
    assert_eq!(v, "42");
}

#[test]
fn concurrent_interleaves_deterministically() {
    let host = topaz_interp::TestHost::new();
    run_with_host(
        "concurrent {\n    a: print(\"a\")\n    b: print(\"b\")\n}",
        &host,
    )
    .expect("runs");
    // Textual order under the reference scheduler.
    assert_eq!(host.stdout(), vec!["a", "b"]);
}

#[test]
fn concurrent_timeout_millisecond_overflow_is_a_guard() {
    let (code, message) =
        run_error("concurrent(timeout: 307445734561826m) {\n    a: 1\n} else {\n    { a: 0 }\n}");
    assert_eq!(code, codes::GUARD_TYPE);
    assert_eq!(
        message,
        "`concurrent` timeout duration must fit in u64 milliseconds (§15)"
    );
}

#[test]
fn defer_contains_faults_and_control_flow() {
    // A fault inside a deferred action goes to the policy hook and
    // never aborts the program (§13/§14 ruling).
    let host = topaz_interp::TestHost::new();
    let (v, _) = run_with_host("{\n    defer { 1 / 0 }\n    print(\"body\")\n}\n7", &host)
        .expect("contained");
    assert_eq!(v, "7");
    assert_eq!(host.stdout(), vec!["body"]);
    assert_eq!(host.defer_errors().len(), 1);
    assert!(host.defer_errors()[0].contains("TPZ4002"));

    // Escaping control flow from a deferred block cannot cancel the
    // in-flight unwind.
    let host2 = topaz_interp::TestHost::new();
    let (v2, _) = run_with_host(
        "function f() -> int {\n    defer { return 99 }\n    return 1\n}\nf()",
        &host2,
    )
    .expect("runs");
    assert_eq!(v2, "1");
    assert_eq!(host2.defer_errors().len(), 1);
}

#[test]
fn faulted_deferred_call_restores_type_parameter_context() {
    let host = topaz_interp::TestHost::new();
    let (_, stdout) = run_with_host(
        "function poison<Leaked>(value: Leaked) -> int {\n    return 1 / 0\n}\nfunction run() -> int {\n    defer {\n        match 41 {\n        case value: Leaked => print(\"leaked\")\n        case _ => print(\"clean\")\n        }\n    }\n    defer { poison(41) }\n    0\n}\nrun()",
        &host,
    )
    .expect("a contained deferred fault restores the enclosing type context");
    assert!(stdout.is_empty());
    assert_eq!(
        host.defer_errors(),
        vec![
            "TPZ4002: integer division by zero",
            "TPZ5099: `Leaked` is not a runtime type"
        ]
    );
}

#[test]
fn omitted_range_step_is_one() {
    // §10: omitted `by` is always step 1 — `5..1` is empty.
    assert_eq!(run_value("for i in 5..1 { i }"), "[]");
    assert_eq!(run_value("for i in 5..1 by -1 { i }"), "[5, 4, 3, 2, 1]");
}

#[test]
fn value_for_rejects_break_and_continue() {
    assert_eq!(
        run_error("let xs = for i in 1..3 { if i == 2 { break }\n i }\nxs").0,
        codes::GUARD_TYPE
    );
    // Statement-position for may still break.
    let host = topaz_interp::TestHost::new();
    run_with_host(
        "for i in 1..5 {\n    if i == 3 { break }\n    print(\"{i}\")\n}",
        &host,
    )
    .expect("runs");
    assert_eq!(host.stdout(), vec!["1", "2"]);
}

#[test]
fn type_pattern_conformance_landed() {
    // The §6 runtime conformance surface executes — container
    // types check their payloads, mismatches fall through.
    assert_eq!(
        run_value("match Some(1) {\n    case x: Option<int> => 1\n    case _ => 0\n}"),
        "1"
    );
    assert_eq!(
        run_value("match Some(\"s\") {\n    case x: Option<int> => 1\n    case _ => 0\n}"),
        "0"
    );
}

#[test]
fn to_int_trims() {
    // Runtime policy is strict because a nullable plain member can fault.
    assert_eq!(run_value("toInt(\" 42 \") ?? 0"), "42");
}

#[test]
fn expiry_beats_later_arm_fault() {
    // Per-arm post-quantum deadline checks: once the slow first arm
    // expires the timeout, the later faulting arm is abandoned and
    // its fault unobserved (§15).
    let host = topaz_interp::TestHost::new();
    host.set_tick_per_poll(10);
    let (v, _) = run_with_host(
        "function spin() -> int {\n    let mut i = 0\n    while true { i += 1 }\n    return i\n}\nlet r = concurrent(timeout: 5ms) {\n    slow: spin()\n    boom: 1 / 0\n} else { { slow: 0, boom: 0 } }\nr.boom",
        &host,
    )
    .expect("expiry wins over the later fault");
    assert_eq!(v, "0");
}

#[test]
fn named_arguments_fill_parameter_slots() {
    assert_eq!(
        run_value(
            "function greet(name: string, salutation: string = \"Hello\") -> string {\n    return \"{salutation}, {name}\"\n}\nif greet(\"T\", salutation: \"Hi\") == \"Hi, T\" { 1 } else { 0 }"
        ),
        "1"
    );
    assert_eq!(
        run_error("function f(a: int) -> int {\n    return a\n}\nf(1, a: 2)").0,
        codes::GUARD_ARITY
    );
    assert_eq!(
        run_error("function f(a: int) -> int {\n    return a\n}\nf(b: 2)").0,
        codes::GUARD_ARITY
    );
}

#[test]
fn spread_arguments_splice_arrays() {
    assert_eq!(
        run_value(
            "function sum(...xs: int) -> int {\n    return reduce(xs, 0, (a: int, b: int) => a + b)\n}\nsum(...Array.of(1, 2), 3)"
        ),
        "6"
    );
    assert_eq!(
        run_error("function f(...xs: int) -> int {\n    return 0\n}\nf(...1)").0,
        codes::GUARD_TYPE
    );
}

#[test]
fn runtime_conformance_covers_aliases_and_containers() {
    assert_eq!(
        run_error("type UserId = int\nlet id: UserId = \"x\"\n0").0,
        codes::GUARD_TYPE
    );
    assert_eq!(
        run_value(
            "type Pair<T> = { first: T, second: T }\nlet b: Pair<int> = { first: 0, second: 1 }\nb.first"
        ),
        "0"
    );
    assert_eq!(
        run_error(
            "let mut m: Map<string, int> = Map.new()\nm.insert(\"k\", 1)\nlet bad: Map<int, int> = m\n0"
        )
        .0,
        codes::GUARD_TYPE
    );
}

#[test]
fn defaults_cannot_reference_parameters() {
    // §7: defaults are const expressions under the DEFINING
    // environment — earlier parameters are not visible.
    assert_eq!(
        run_error("function f(x: int, y: int = x + 1) -> int {\n    return y\n}\nf(41)").0,
        codes::GUARD_TYPE
    );
}

#[test]
fn spread_fills_only_the_variadic_tail() {
    assert_eq!(
        run_error("function add(a: int, b: int) -> int {\n    return a + b\n}\nadd(...[1, 2])").0,
        codes::GUARD_ARITY
    );
    assert_eq!(
        run_error("function head(a: int, ...xs: int) -> int {\n    return a\n}\nhead(...[1, 2])").0,
        codes::GUARD_ARITY
    );
    // Tail-region values after a spread are legal.
    assert_eq!(
        run_value(
            "function head(a: int, ...xs: int) -> int {\n    return a + xs.length\n}\nhead(0, ...[1, 2], 3)"
        ),
        "3"
    );
}

#[test]
fn alias_conformance_cycles_and_nesting() {
    assert_eq!(
        run_error("type A = A\nlet x: A = 1\n0").0,
        codes::GUARD_TYPE
    );
    assert_eq!(
        run_value(
            // ERR-003: a statement ending in `>` needs `;` (§1a).
            "type Id<T> = T\ntype Vec2<T> = Id<Array<T>>;\nlet xs: Vec2<int> = [1]\nxs.length"
        ),
        "1"
    );
    assert_eq!(
        run_value(
            "function f() -> int {\n    type Local = int\n    let x: Local = 1\n    return x\n}\nf()"
        ),
        "1"
    );
}

#[test]
fn function_shape_conformance_is_arity_aware() {
    assert_eq!(
        run_value("match print {\n    case f: () -> int => 1\n    case _ => 0\n}"),
        "0"
    );
    assert_eq!(
        run_value("match print {\n    case f: (string) -> () => 1\n    case _ => 0\n}"),
        "1"
    );
}

#[test]
fn empty_spreads_keep_the_tail_marker() {
    // The r2 counterexample: `...[]` still marks the tail region,
    // so a following value cannot fill a fixed slot.
    assert_eq!(
        run_error("function head(a: int, ...xs: int) -> int {\n    return a\n}\nhead(...[], 1)").0,
        codes::GUARD_ARITY
    );
}

#[test]
fn variadic_shape_conformance_respects_required_minimums() {
    // need(a, ...xs) cannot be called with zero args, so it does
    // not conform to () -> int.
    assert_eq!(
        run_value(
            "function need(a: int, ...xs: int) -> int {\n    return a\n}\nmatch need {\n    case f: () -> int => 1\n    case _ => 0\n}"
        ),
        "0"
    );
    assert_eq!(
        run_value(
            "function need(a: int, ...xs: int) -> int {\n    return a\n}\nmatch need {\n    case f: (int, ...int) -> int => 1\n    case _ => 0\n}"
        ),
        "1"
    );
}

#[test]
fn builtin_calls_take_named_arguments() {
    assert_eq!(run_value("toInt(text: \"42\") ?? 0"), "42");
    assert_eq!(run_error("toInt(nope: \"42\") ?? 0").0, codes::GUARD_ARITY);
}

#[test]
fn named_arguments_to_non_callables_guard() {
    assert_eq!(run_error("let x = 1\nx(s: 2)").0, codes::GUARD_NOT_CALLABLE);
}

// ---- member/index assignment (§4/§5/§9) -----------------------------------

#[test]
fn array_cell_assignment_writes_the_shared_store() {
    assert_eq!(
        run_value("let mut xs = [1, 2, 3]\nlet ys = xs\nxs[1] = 20\nxs[2] += 5\nys"),
        "[1, 20, 8]"
    );
    assert_eq!(
        run_error("let mut xs = [1]\nxs[3] = 0").0,
        codes::FAULT_INDEX
    );
    assert_eq!(
        run_error("let mut m = Map.new()\nm[\"k\"] = 1").0,
        codes::GUARD_TYPE
    );
}

#[test]
fn record_path_assignment_is_a_functional_update() {
    assert_eq!(
        run_value(
            "let mut r = { a: 1, b: { c: 2 } }\nlet old = r\nr.a = 10\nr.b.c += 1\n\"{r.a} {r.b.c} {old.a}\""
        ),
        "10 3 1"
    );
    assert_eq!(
        run_error("let r = { a: 1 }\nr.a = 2").0,
        codes::GUARD_IMMUTABLE
    );
    assert_eq!(
        run_error("let mut r = { a: 1 }\nr.nope = 2").0,
        codes::GUARD_NO_FIELD
    );
}

#[test]
fn coalesce_assignment_through_paths_writes_only_none() {
    assert_eq!(
        run_value(
            "let mut rs = [{ n: None }, { n: Some(1) }]\nrs[0].n ??= Some(7)\nrs[1].n ??= Some(9)\n\"{rs[0].n ?? 0} {rs[1].n ?? 0}\""
        ),
        "7 1"
    );
}

#[test]
fn spec_5_argument_ordering_at_runtime() {
    // §5: named arguments follow ALL positional and spread
    // arguments; named-after-spread is the required order.
    assert_eq!(
        run_value(
            "function f(tag: string = \"x\", ...xs: int) -> int {\n    return xs.length\n}\nf(...[1, 2], tag: \"t\")"
        ),
        "2"
    );
    assert_eq!(
        run_error(
            "function f(tag: string = \"x\", ...xs: int) -> int {\n    return xs.length\n}\nf(tag: \"t\", ...[1, 2])"
        )
        .0,
        codes::GUARD_ARITY
    );
    // A spread cannot skip an unsatisfied fixed parameter.
    assert_eq!(
        run_error("function g(a: int, ...xs: int) -> int {\n    return a\n}\ng(...[1, 2], a: 0)").0,
        codes::GUARD_ARITY
    );
}

#[test]
fn compound_assignment_reads_the_target_before_the_rhs() {
    // §2: read-operation-write, left to right — the pre-RHS read is
    // the left operand even when the RHS mutates the target.
    assert_eq!(
        run_value(
            "let mut xs = [1]\nfunction rhs() -> int {\n    xs[0] = 10\n    return 2\n}\nxs[0] += rhs()\nxs[0]"
        ),
        "3"
    );
    assert_eq!(
        run_value(
            "let mut x = 1\nfunction rhs() -> int {\n    x = 10\n    return 2\n}\nx += rhs()\nx"
        ),
        "3"
    );
    // A record path: the leaf read is pre-RHS, but the functional
    // rebuild starts from the root as it stands AFTER the RHS, so a
    // sibling write made by the RHS survives.
    assert_eq!(
        run_value(
            "let mut rs = [{ a: 1, b: 0 }]\nfunction rhs() -> int {\n    rs[0].b = 9\n    return 2\n}\nrs[0].a += rhs()\n\"{rs[0].a} {rs[0].b}\""
        ),
        "3 9"
    );
}

#[test]
fn spread_cannot_skip_a_non_leading_required_parameter() {
    assert_eq!(
        run_error(
            "function f(a: int = 0, b: int, ...xs: int) -> int {\n    return a + b + xs.length\n}\nf(0, ...[5], b: 2)"
        )
        .0,
        codes::GUARD_ARITY
    );
}

#[test]
fn compound_assignment_to_an_unbound_target_fails_before_the_rhs() {
    let host = topaz_interp::TestHost::new();
    let r = run_with_host(
        "function rhs() -> int {\n    print(\"rhs ran\")\n    return 1\n}\nmissing += rhs()",
        &host,
    );
    let (code, _) = r.expect_err("unbound target");
    assert_eq!(code, codes::GUARD_UNBOUND);
    assert!(host.stdout().is_empty(), "the RHS must not run");
}

#[test]
fn mutator_methods_fault_without_a_mut_root() {
    // §9: in-place collection mutation requires a mutable binding.
    assert_eq!(
        run_error("let xs = [1]\nxs.push(2)").0,
        codes::GUARD_IMMUTABLE
    );
    assert_eq!(run_value("let mut xs = [1]\nxs.push(2)\nxs.length"), "2");
    assert_eq!(
        run_error("let xs = [1, 2]\nxs[0] = 9").0,
        codes::GUARD_IMMUTABLE
    );
    assert_eq!(
        run_error("let m = Map.new()\nm.insert(\"k\", 1)").0,
        codes::GUARD_IMMUTABLE
    );
    assert_eq!(
        run_error("let buffer = ByteBuffer.allocate(1)\nbuffer.set(0, 255)").0,
        codes::GUARD_IMMUTABLE
    );
    assert_eq!(
        run_value("let mut buffer = ByteBuffer.allocate(1)\nbuffer.set(0, 255)\nbuffer.get(0)"),
        "255"
    );
    // A shared alias is its own immutable binding (§9 is per-binding).
    assert_eq!(
        run_error("let mut xs = [1]\nlet ys = xs\nys.push(2)").0,
        codes::GUARD_IMMUTABLE
    );
    // A record field named like a mutator is not a §9 mutation.
    assert_eq!(
        run_value("let r = { remove: (x: int) => x }\nr.remove(5)"),
        "5"
    );
}

#[test]
fn mutator_root_check_sees_through_all_forms() {
    // §9 regression folds (runtime): parens, first-class values, optional
    // access, and record-stored handles all key off the true root.
    assert_eq!(
        run_error("let xs = [1]\n(xs).push(2)").0,
        codes::GUARD_IMMUTABLE
    );
    assert_eq!(
        run_error("let ys = [1, 2]\n(ys)[0] = 9").0,
        codes::GUARD_IMMUTABLE
    );
    assert_eq!(
        run_error("let xs = [1]\nlet f = xs.push\nf(2)").0,
        codes::GUARD_IMMUTABLE
    );
    assert_eq!(
        run_value("let mut xs = [1]\nlet f = xs.push\nf(2)\nxs.length"),
        "2"
    );
    assert_eq!(
        run_error("let xs: Option<Array<int>> = Some([1])\nxs?.push(2)").0,
        codes::GUARD_IMMUTABLE
    );
    // The handle is checked at acquisition; a record field named
    // `push` over a MUTABLE collection still works.
    assert_eq!(
        run_error("let xs = [1]\nlet r = { push: xs.push }\nr.push(2)").0,
        codes::GUARD_IMMUTABLE
    );
    assert_eq!(
        run_value("let mut xs = [1]\nlet r = { push: xs.push }\nr.push(2)\nxs.length"),
        "2"
    );
}

#[test]
fn pipe_field_mutator_requires_a_mut_root() {
    // §9: a mutator obtained via `coll |> .push` is checked too.
    assert_eq!(
        run_error("let xs = [1]\nlet f = xs |> .push\nf(2)").0,
        codes::GUARD_IMMUTABLE
    );
    assert_eq!(
        run_value("let mut xs = [1]\nlet f = xs |> .push\nf(2)\nxs.length"),
        "2"
    );
}

#[test]
fn optional_mutator_is_value_dependent() {
    // §9 on an optional receiver: None short-circuits (no mutation,
    // no fault — agrees with the checker staying silent); a Some
    // receiver through an immutable root faults at acquisition.
    assert_eq!(
        run_value("let xs: Option<Array<int>> = None\nlet f = xs?.push\nf ?? 0"),
        "0"
    );
    assert_eq!(
        run_error("let xs: Option<Array<int>> = Some([1])\nlet f = xs?.push\nf").0,
        codes::GUARD_IMMUTABLE
    );
}

#[test]
fn optional_calls_short_circuit_and_flatten() {
    // §12: None short-circuits — the method and its args are not
    // evaluated (so an immutable root never trips §9 on None).
    assert_eq!(
        run_value("let xs: Option<Array<int>> = None\nxs?.push(2)\n\"done\""),
        "done"
    );
    assert_eq!(
        run_value("let xs: Option<Array<int>> = None\nxs?.get(0) ?? -1"),
        "-1"
    );
    // An Option-returning method flat-maps (no double Some).
    assert_eq!(
        run_value("let xs: Option<Array<int>> = Some([10, 20])\nxs?.get(0) ?? -1"),
        "10"
    );
    assert_eq!(
        run_value("let xs: Option<Array<int>> = Some([10])\nxs?.get(5) ?? -1"),
        "-1"
    );
    // A non-Option-returning method maps (result wraps in Some); the
    // mutation reaches the array inside the Some.
    assert_eq!(
        run_value("let mut xs: Option<Array<int>> = Some([1])\nxs?.push(2)\nxs?.get(1) ?? -1"),
        "2"
    );
}

#[test]
fn nested_optional_mutator_anchors_at_the_root() {
    // §9 through a nested optional chain still anchors at the root
    // binding: r is immutable, so the Some/Some branch faults.
    assert_eq!(
        run_error(
            "let r: Option<{ xs: Option<Array<int>> }> = Some({ xs: Some([1]) })\nr?.xs?.push(2)"
        )
        .0,
        codes::GUARD_IMMUTABLE
    );
    // A mutable root mutates through the chain.
    assert_eq!(
        run_value(
            "let mut r: Option<{ xs: Option<Array<int>> }> = Some({ xs: Some([1]) })\nr?.xs?.push(2)\nr?.xs?.get(1) ?? -1"
        ),
        "2"
    );
    // None anywhere short-circuits — no mutation, no §9 fault.
    assert_eq!(
        run_value("let r: Option<{ xs: Option<Array<int>> }> = None\nr?.xs?.push(2)\n\"ok\""),
        "ok"
    );
}

#[test]
fn assignment_through_optional_access_faults() {
    // §4: an index target whose object routes through `?.` is
    // rejected the same on both engines, regardless of the runtime
    // None/Some value (the checker rejects it statically too).
    assert_eq!(
        run_error("let mut r: Option<{ xs: Array<int> }> = None\nr?.xs[0] = 1").0,
        codes::GUARD_TYPE
    );
    assert_eq!(
        run_error("let mut r: Option<{ xs: Array<int> }> = Some({ xs: [1] })\nr?.xs[0] = 1").0,
        codes::GUARD_TYPE
    );
}

#[test]
fn pipe_into_call_inserts_and_substitutes() {
    // §11 first-argument insertion: the piped value is the call's
    // first positional.
    assert_eq!(
        run_value("function add(a: int, b: int) -> int {\n    return a + b\n}\n1 |> add(2)"),
        "3"
    );
    // Into a method call (`??` binds tighter than `|>`, so the pipe
    // stage is parenthesized).
    assert_eq!(run_value("let xs = [10, 20]\n(0 |> xs.get()) ?? -1"), "10");
    // §11 placeholder replacement suppresses first-arg insertion and
    // binds `_` to the piped value (anywhere in the args).
    assert_eq!(
        run_value("let xs = [1, 2, 3]\nxs |> map(_, (x: int) => x * 2)"),
        "[2, 4, 6]"
    );
    assert_eq!(
        run_value("let xs = [1, 2, 3, 4]\nxs |> reduce(_, 0, (a: int, b: int) => a + b)"),
        "10"
    );
    // Into an optional call (§12 container preserved).
    assert_eq!(
        run_value("let xs: Option<Array<int>> = Some([10])\n(0 |> xs?.get()) ?? -1"),
        "10"
    );
    // Unary application still chains.
    assert_eq!(
        run_value(
            "function inc(n: int) -> int {\n    return n + 1\n}\nfunction dbl(n: int) -> int {\n    return n * 2\n}\n3 |> inc |> dbl"
        ),
        "8"
    );
    // `_` outside a pipeline stage is an error.
    assert_eq!(run_error("let x = _\nx").0, codes::GUARD_TYPE);
}

#[test]
fn pipe_placeholder_binds_in_scope_and_is_isolated() {
    // §11: `_` is bound in a child scope for the stage, so a lambda
    // that escapes the stage still captures it.
    assert_eq!(
        run_value(
            "function keep(f: (int) -> int) -> (int) -> int {\n    return f\n}\nlet f = 5 |> keep((x: int) => x + _)\nf(10)"
        ),
        "15"
    );
    // `_` inside a lambda invoked DURING the stage also binds.
    assert_eq!(
        run_value(
            "function apply(f: (int) -> int) -> int {\n    return f(10)\n}\n5 |> apply((x: int) => x + _)"
        ),
        "15"
    );
    // Nested pipes: the innermost `_` shadows the outer one.
    assert_eq!(
        run_value(
            "function add(a: int, b: int) -> int {\n    return a + b\n}\n10 |> add(_, 100 |> add(_, 1))"
        ),
        "111"
    );
    // A named argument targeting the piped slot is a
    // duplicate (§5/§11), the same on both engines.
    assert_eq!(
        run_error("function id(a: int) -> int {\n    return a\n}\n1 |> id(a: 2)").0,
        codes::GUARD_ARITY
    );
    // Unwind isolation: a deferred
    // placeholder stage that faults leaves no `_` binding behind —
    // a later `_` outside a pipe still faults.
    assert_eq!(
        run_error(
            "function g() -> int {\n    defer { let z = [9] |> map(_, (x: int) => 1 / 0)\n    }\n    return 0\n}\ng()\nlet x = _\nx"
        )
        .0,
        codes::GUARD_TYPE
    );
    // `_` outside any pipeline stage is an error.
    assert_eq!(run_error("let x = _\nx").0, codes::GUARD_TYPE);
}

#[test]
fn pipe_placeholder_search_covers_argument_expressions_and_isolates_nested_stages() {
    assert_eq!(
        run_value(concat!(
            "function identity(value: int) -> int { value }\n",
            "function render(value: string) -> string { value }\n",
            "function add(left: int, right: int) -> int { left + right }\n",
            "let block = 5 |> identity({ _ })\n",
            "let branch = 6 |> identity(if true { _ } else { 0 })\n",
            "let text = 7 |> render(\"value {_}\")\n",
            "let nested = 10 |> add(100 |> add(_, 1))\n",
            "\"{block}:{branch}:{text}:{nested}\"",
        )),
        "5:6:value 7:111",
    );
}

#[test]
fn faulted_deferred_pipe_stage_does_not_skip_later_defers() {
    // R3 finding: a deferred placeholder stage that faults must not
    // strand a child scope and skip earlier-registered defers.
    let host = topaz_interp::TestHost::new();
    let _ = run_with_host(
        "function sink(x: int) -> int {\n    return x\n}\ndefer print(\"first\")\ndefer { let z = sink(1 |> sink(_ + (1 / 0)))\n }\nprint(\"main\")",
        &host,
    );
    assert_eq!(host.stdout(), vec!["main", "first"]);
    assert_eq!(
        host.defer_errors(),
        vec!["TPZ4002: integer division by zero"]
    );
    // A `_` in a concurrent arm stage stays isolated to the arm.
    assert_eq!(
        run_error(
            "function add(a: int, b: int) -> int {\n    return a + b\n}\nlet r = concurrent {\n    a: 5 |> add(_, 0)\n}\nlet x = _\nx"
        )
        .0,
        codes::GUARD_TYPE
    );
}

#[test]
fn a_faulting_block_does_not_drain_its_defers_consistently() {
    // §14 (established, CDR-006): a fault is NOT a scope exit, so a
    // block that faults does not run its own defers — the same
    // whether the block is top-level or is itself a deferred action.
    let top = topaz_interp::TestHost::new();
    let _ = run_with_host(
        "{ defer print(\"inner\")\n    1 / 0\n}\nprint(\"after\")",
        &top,
    );
    assert!(top.stdout().is_empty());
    let in_defer = topaz_interp::TestHost::new();
    let _ = run_with_host(
        "defer {\n    defer print(\"inner\")\n    1 / 0\n}\nprint(\"main\")",
        &in_defer,
    );
    assert_eq!(in_defer.stdout(), vec!["main"]);
    // A NORMAL block exit, deferred or not, DOES drain its defers.
    let normal = topaz_interp::TestHost::new();
    let _ = run_with_host(
        "defer {\n    defer print(\"inner\")\n    print(\"body\")\n}\nprint(\"main\")",
        &normal,
    );
    assert_eq!(normal.stdout(), vec!["main", "body", "inner"]);
}

#[test]
fn placeholder_as_pipe_callee_is_rejected() {
    // §11: `_` is valid only in a stage call's argument list, not as
    // the callee — a static error on both engines.
    assert_eq!(
        run_error("function echo(s: int) -> int {\n    return s\n}\necho |> _(1)").0,
        codes::GUARD_TYPE
    );
}

// ---- §22.2 Option→Result bridge (okOr / okOrElse) --------------------

#[test]
fn ok_or_bridges_option_to_result_eagerly() {
    // EAGER: `Some(v)->Ok(v)` (the error is ignored), `None->Err(error)`.
    assert_eq!(run_value("Some(5).okOr(\"e\")"), "Ok(5)");
    assert_eq!(run_value("(None).okOr(\"e\")"), "Err(e)");
    // `toInt(_).okOr(_)?` is the intended use — the bridge feeds `?`.
    assert_eq!(run_value("toInt(\"7\").okOr(\"nan\")"), "Ok(7)");
    assert_eq!(run_value("toInt(\"x\").okOr(\"nan\")"), "Err(nan)");
}

#[test]
fn ok_or_else_bridges_option_to_result_lazily() {
    // `Some(v)->Ok(v)`, `None->Err(f())`.
    assert_eq!(run_value("Some(5).okOrElse(() => \"e\")"), "Ok(5)");
    assert_eq!(run_value("(None).okOrElse(() => \"e\")"), "Err(e)");
}

#[test]
fn ok_or_else_callback_is_lazy_on_some_but_fires_on_none() {
    // THE LAZY PROOF: the callback's `print` MUST NOT fire for a `Some`
    // receiver (the closure is constructed but never CALLED), and MUST fire
    // exactly once for a `None` receiver.
    let some = topaz_interp::TestHost::new();
    let (v, _) =
        run_with_host("let r = Some(5).okOrElse(() => print(\"X\"))\nr", &some).expect("runs");
    assert_eq!(v, "Ok(5)");
    assert_eq!(some.stdout(), Vec::<String>::new(), "Some must not call f");

    let none = topaz_interp::TestHost::new();
    let (v2, _) =
        run_with_host("let r = (None).okOrElse(() => print(\"X\"))\nr", &none).expect("runs");
    // `print` returns `()`, so the wrapped error is `Err(())`.
    assert_eq!(v2, "Err(())");
    assert_eq!(none.stdout(), vec!["X"], "None must call f exactly once");
}

#[test]
fn bridge_on_a_non_option_receiver_faults_no_member() {
    // The receiver preflight: a non-Option receiver has no such member.
    assert_eq!(
        run_error("let n = 5\nn.okOr(\"e\")").0,
        codes::GUARD_NO_FIELD
    );
    assert_eq!(
        run_error("let n = 5\nn.okOrElse(() => \"e\")").0,
        codes::GUARD_NO_FIELD
    );
}

#[test]
fn bridge_arity_faults() {
    // Both take exactly one argument.
    assert_eq!(run_error("Some(5).okOr()").0, codes::GUARD_ARITY);
    assert_eq!(
        run_error("(None).okOrElse(() => 1, () => 2)").0,
        codes::GUARD_ARITY
    );
}
