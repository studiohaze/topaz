use super::*;

#[test]
fn emits_a_match_return_arm() {
    // §5 a `case … => return e` arm (inside a function/lambda) lowers to
    // `return Ok(e)`; a bare `return` to `return Ok(Value::Unit)`.
    let src = emit_unit(&unit_of(
        "let f = (x) => { match x { case 0 => return 100\ncase _ => 1 } }\nf(0)",
    ))
    .expect("emit");
    assert!(src.contains("return Ok("), "got:\n{src}");
    // A top-level match `return` arm is refused (it would runtime-fault
    // "return outside a function").
    assert_eq!(
        emit_unit(&unit_of("match 0 { case 0 => return 1\ncase _ => 2 }")),
        Err(EmitError::unsupported("return outside a function"))
    );
}

#[test]
fn emits_array_indexing() {
    // §1 `object[index]` → the shared `index_value` leaf (object then
    // index, by value).
    let src = emit_unit(&unit_of("[10, 20, 30][1]")).expect("emit");
    assert!(
        src.contains(
            "index_value(Value::array(vec![Value::Int(10), Value::Int(20), Value::Int(30)]), Value::Int(1),"
        ),
        "got:\n{src}"
    );
}

#[test]
fn emits_index_assign_into_a_mutable_array() {
    // §9 `xs[i] = v`: object then index then the shared `index_slot`
    // leaf (bounds/type fault BEFORE the RHS), then the RHS, then the
    // in-place `borrow_mut` write.
    let src = emit_unit(&unit_of("let mut xs = [1, 2, 3]\nxs[0] = 9\nxs")).expect("emit");
    assert!(
        src.contains("let (__ia_store, __ia_k) = index_slot(&__ia_base, &__ia_idx,"),
        "got:\n{src}"
    );
    assert!(
        src.contains("let __ia_v = Value::Int(9); __ia_store.borrow_mut()[__ia_k] = __ia_v;"),
        "got:\n{src}"
    );
}

#[test]
fn emits_compound_index_assign_reading_the_element_first() {
    // §2 a compound `xs[i] += v` reads the current element (after the bounds
    // check, before the RHS) and combines through the shared `binary_value`
    // leaf, then writes.
    let src = emit_unit(&unit_of("let mut xs = [1, 2, 3]\nxs[2] += 5\nxs")).expect("emit");
    assert!(
        src.contains("let __ia_cur = __ia_store.borrow()[__ia_k].clone();"),
        "got:\n{src}"
    );
    assert!(
        src.contains("let __ia_v = Value::Int(5); let __ia_new = binary_value(BinaryOp::Add, __ia_cur, __ia_v,"),
        "got:\n{src}"
    );
}

#[test]
fn index_assign_to_an_immutable_root_emits_a_guard_immutable_fault() {
    // The root `xs` is an immutable `let` — the interpreter faults
    // GUARD_IMMUTABLE in `schedule_path_assign` BEFORE evaluating the
    // index/RHS. The emitter now emits that exact fault (run≡build) instead
    // of refusing, with no base/index/RHS evaluation.
    let src = emit_unit(&unit_of("let xs = [1, 2]\nxs[0] = 9\nxs")).expect("emit");
    assert!(
        src.contains(
            "return Err(fault(codes::GUARD_IMMUTABLE, \"`xs` is not `let mut` and cannot be assigned\","
        ),
        "got:\n{src}"
    );
}

#[test]
fn record_path_assign_rebuilds_and_rebinds_the_root() {
    // A record-path assign `r.a = v` rebuilds the record through the shared
    // `update_fields_value` leaf and rebinds the mutable root — matching
    // the interpreter's `apply_record_path`.
    let src = emit_unit(&unit_of("let mut r = { a: 1 }\nr.a = 2\nr")).expect("emit");
    assert!(
        src.contains("update_fields_value(&__rp_root, &[\"a\"], __rp_v,"),
        "got:\n{src}"
    );
}

#[test]
fn record_path_assign_to_an_immutable_root_emits_a_guard_immutable_fault() {
    // An immutable record-path root faults GUARD_IMMUTABLE, like an immutable
    // index-assign root.
    let src = emit_unit(&unit_of("let r = { a: 1 }\nr.a = 2\nr")).expect("emit");
    assert!(
        src.contains(
            "return Err(fault(codes::GUARD_IMMUTABLE, \"`r` is not `let mut` and cannot be assigned\","
        ),
        "got:\n{src}"
    );
}

#[test]
fn emits_a_cell_path_assign() {
    // §4/§9 a cell-path assign `xs[i].f = v` resolves the array slot in place
    // (`index_slot`) and rebuilds the element through the shared
    // `update_fields_value` leaf, writing it back — matching the interpreter's
    // `apply_cell_path`.
    let src = emit_unit(&unit_of("let mut xs = [{ a: 1 }]\nxs[0].a = 9\nxs[0]")).expect("emit");
    assert!(
        src.contains("update_fields_value(&__cp_cur, &[\"a\"], __cp_v,"),
        "got:\n{src}"
    );
    assert!(
        src.contains("__cp_store.borrow_mut()[__cp_k] = __cp_new;"),
        "got:\n{src}"
    );
}

#[test]
fn a_cell_path_compound_reads_the_leaf_before_the_rhs() {
    // A compound `xs[i].f += v` reads the leaf (`walk_fields_value`) before the
    // RHS, combines through `binary_value`, then re-reads the element and writes.
    let src = emit_unit(&unit_of("let mut xs = [{ a: 10 }]\nxs[0].a += 5\nxs[0]")).expect("emit");
    assert!(
        src.contains("let __cp_old = { let (__cp_s, __cp_i) = index_slot"),
        "got:\n{src}"
    );
    assert!(
        src.contains("binary_value(BinaryOp::Add, __cp_old, __cp_v,"),
        "got:\n{src}"
    );
}

#[test]
fn a_cell_path_immutable_root_emits_a_guard_immutable_fault() {
    // An immutable array root faults GUARD_IMMUTABLE before any base/index/RHS
    // evaluation, like the index-assignment and record-path roots.
    let src = emit_unit(&unit_of("let xs = [{ a: 1 }]\nxs[0].a = 9")).expect("emit");
    assert!(
        src.contains(
            "return Err(fault(codes::GUARD_IMMUTABLE, \"`xs` is not `let mut` and cannot be assigned\","
        ),
        "got:\n{src}"
    );
}

#[test]
fn emits_map_through_the_shared_callback_hof_driver() {
    let src = emit_unit(&unit_of("map([1, 2], (x) => x * 2)")).expect("emit");
    assert!(
        src.contains("call_callback_hof(CallbackHofKind::Map"),
        "got:\n{src}"
    );
}

#[test]
fn emits_filter_through_the_shared_callback_hof_driver() {
    let src = emit_unit(&unit_of("filter([1, 2], (x) => x > 0)")).expect("emit");
    assert!(
        src.contains("call_callback_hof(CallbackHofKind::Filter"),
        "got:\n{src}"
    );
}

#[test]
fn emits_reduce_through_the_shared_callback_hof_driver() {
    let src = emit_unit(&unit_of("reduce([1, 2], 0, (a, x) => a + x)")).expect("emit");
    assert!(
        src.contains("call_callback_hof(CallbackHofKind::Reduce"),
        "got:\n{src}"
    );
}

#[test]
fn emits_receiver_hof_spread_faults() {
    let map = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
[1].map(...mark("map", [0]))
"#,
    ))
    .expect("receiver map spread fault emits");
    assert!(
        map.contains("member_value(&__recv, \"map\",")
            && map.contains("call_value_spread(__field")
            && map.contains("__tpz_recv_spread")
            && map.contains("spread arguments require a variadic parameter"),
        "got:\n{map}"
    );

    let flat_map = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
Some(1).flatMap(...mark("flat", [0]))
"#,
    ))
    .expect("receiver flatMap spread fault emits");
    assert!(
        flat_map.contains("member_value(&__recv, \"flatMap\",")
            && flat_map.contains("call_value_spread(__field")
            && flat_map.contains("__tpz_recv_spread"),
        "got:\n{flat_map}"
    );

    let ok_or_else = emit_unit(&unit_of(
        r#"
function mark(label: string, xs: Array<int>) -> Array<int> {
print(label)
xs
}
None.okOrElse(...mark("ok", [0]))
"#,
    ))
    .expect("receiver okOrElse spread fault emits");
    assert!(
        ok_or_else.contains("member_value(&__recv, \"okOrElse\",")
            && ok_or_else.contains("call_value_spread(__field")
            && ok_or_else.contains("__tpz_recv_spread"),
        "got:\n{ok_or_else}"
    );
}

#[test]
fn a_hof_with_the_wrong_argument_count_is_unsupported() {
    // `map` needs exactly two positional arguments.
    assert_eq!(
        emit_unit(&unit_of("map([1])")),
        Err(EmitError::unsupported("builtin call shape"))
    );
}

#[test]
fn emits_an_indirect_closure_call() {
    // Calling a closure-VALUED local now lowers through `call_value`
    // (which checks arity at runtime via the ABI's `arity` /
    // `param_name`). `f` mangles to `_t_66`.
    let src = emit_unit(&unit_of("let f = (x) => x + 1\nf(2)")).expect("emit");
    assert!(
        src.contains("call_value(_t_66.clone(), vec![Value::Int(2)]"),
        "got:\n{src}"
    );
}

#[test]
fn a_lambda_arity_mismatch_now_emits_and_faults_at_runtime() {
    // The emit-time arity check is gone — `call_value` raises the §5
    // arity fault at runtime (with the call span), like the
    // interpreter, so a mismatch is emitted, not refused. The param
    // names ride along for the "missing argument for parameter" fault.
    let src = emit_unit(&unit_of("((x, y) => x + y)(1)")).expect("emit");
    assert!(src.contains("call_value("), "got:\n{src}");
    assert!(src.contains("params: &[\"x\", \"y\"]"), "got:\n{src}");
}

#[test]
fn emits_block_as_a_rust_block_value() {
    // A block expression is its own scope and yields its tail value:
    // a Rust block assigned to the shared initialization value, binding the local and returning the
    // tail. (Statement indentation inside a nested block is cosmetic;
    // the differential harness proves it compiles and runs.)
    let src = emit_unit(&unit_of("{ let x = 1\nx + 1 }")).expect("emit");
    assert!(
        src.contains("let __topaz_init_value = {") || src.contains("let __topaz_init_value = ({"),
        "got:\n{src}"
    );
    assert!(src.contains("let _t_78 = Value::Int(1);"), "got:\n{src}");
    assert!(
        src.contains("binary_value(BinaryOp::Add, _t_78.clone(), Value::Int(1)"),
        "got:\n{src}"
    );
}

#[test]
fn emits_if_with_the_shared_condition_guard() {
    // The condition runs through the SHARED `condition_bool` (so a
    // non-`bool` faults identically), the `if` span is threaded, and
    // each arm is a block value.
    let src = emit_unit(&unit_of("if true { 1 } else { 2 }")).expect("emit");
    assert!(
        src.contains("if condition_bool(&Value::Bool(true), \"if\", Span::new(FileId("),
        "got:\n{src}"
    );
    assert!(
        src.contains("{ Value::Int(1) } else { Value::Int(2) }"),
        "got:\n{src}"
    );
}

#[test]
fn if_without_else_yields_unit() {
    // A missing `else` is the `Unit` arm — matching the interpreter,
    // whose `KIf` pushes `Value::Unit` on the false branch.
    let src = emit_unit(&unit_of("if true { 1 }")).expect("emit");
    assert!(src.contains("else { Value::Unit }"), "got:\n{src}");
}

#[test]
fn else_if_chains_as_nested_if_expressions() {
    // `else if` lowers because the else branch is itself an `if`
    // expression — a Rust `else if` chain, no special-casing.
    let src = emit_unit(&unit_of("if false { 1 } else if true { 2 } else { 3 }")).expect("emit");
    assert!(
        src.contains("} else if condition_bool(&Value::Bool(true), \"if\","),
        "got:\n{src}"
    );
}

#[test]
fn a_child_scope_may_shadow_without_a_redeclaration_error() {
    // Same-scope redeclaration is refused, but a NESTED block may
    // shadow an enclosing binding (legal §4) — both mangle to the
    // same Rust local, which is exactly what Rust shadowing wants.
    let src = emit_unit(&unit_of("let x = 1\n{ let x = 2\nx }")).expect("emit");
    assert!(src.contains("let _t_78 = Value::Int(1);"), "got:\n{src}");
    // The inner block re-binds the SAME mangled local (Rust shadowing
    // does the rest); the block reads it and closes.
    assert!(src.contains("let _t_78 = Value::Int(2);"), "got:\n{src}");
    assert!(src.contains("_t_78.clone() }"), "got:\n{src}");
}

#[test]
fn block_locals_do_not_escape_the_block() {
    // A block's `let` is scoped to the block; reading it afterwards
    // is a free name (a static error), not a leak.
    assert_eq!(
        emit_unit(&unit_of("{ let x = 1 }\nx")),
        Err(EmitError::unsupported("free identifier"))
    );
}

#[test]
fn a_non_bool_if_condition_is_still_emitted_and_guarded_at_runtime() {
    // The emitter does not itself reject a non-`bool` condition — the
    // SHARED `condition_bool` guard faults it at runtime, identically
    // to the interpreter (the differential harness proves the match).
    let src = emit_unit(&unit_of("if 1 { 2 } else { 3 }")).expect("emit");
    assert!(
        src.contains("condition_bool(&Value::Int(1), \"if\","),
        "got:\n{src}"
    );
}

#[test]
fn emits_while_as_a_rust_while_with_the_shared_guard() {
    // `while` re-tests the condition through the shared guard (keyword
    // `"while"`, the WHOLE statement's span) and the body is a `()`
    // block (the value is discarded — `while` is a statement).
    let src = emit_unit(&unit_of("let mut i = 0\nwhile i < 3 { i = i + 1 }\ni")).expect("emit");
    assert!(
        src.contains(
            "while condition_bool(&binary_value(BinaryOp::Lt, _t_69.clone(), Value::Int(3),"
        ),
        "got:\n{src}"
    );
    assert!(src.contains("\"while\", Span::new(FileId("), "got:\n{src}");
    // The body OPENS with a §15 cooperative checkpoint at the back-edge (so a
    // `concurrent` arm — even one that `continue`s — suspends each iteration);
    // the block is `()`. A tail-less body has no value to discard, so it emits
    // No dead `let _ = Value::Unit;`.
    assert!(src.contains("? { checkpoint().await; "), "got:\n{src}");
    assert!(
        !src.contains("let _ = Value::Unit"),
        "dead Unit store: {src}"
    );
}

#[test]
fn break_and_continue_lower_to_rust_inside_a_loop() {
    let src = emit_unit(&unit_of(
        "while true { if true { break } else { continue } }",
    ))
    .expect("emit");
    assert!(src.contains("break;"), "got:\n{src}");
    assert!(src.contains("continue;"), "got:\n{src}");
}

#[test]
fn break_inside_a_nested_if_is_still_in_loop() {
    // `in_loop` threads through the `if` so a `break` nested in a
    // conditional inside the loop is accepted, not refused.
    let src = emit_unit(&unit_of(
        "let mut i = 0\nwhile true { i = i + 1\nif i >= 2 { break } }\ni",
    ));
    assert!(
        src.is_ok(),
        "break nested in an if inside a loop must emit: {src:?}"
    );
}

#[test]
fn break_outside_a_loop_is_refused() {
    // A `break` with no enclosing loop would emit `break;` that does
    // not compile — refuse (the interpreter / checker rejects it too).
    assert_eq!(
        emit_unit(&unit_of("break")),
        Err(EmitError::unsupported("break outside loop"))
    );
}

#[test]
fn continue_outside_a_loop_is_refused() {
    assert_eq!(
        emit_unit(&unit_of("continue")),
        Err(EmitError::unsupported("continue outside loop"))
    );
}

#[test]
fn a_block_does_not_make_a_top_level_break_in_loop() {
    // A plain block is transparent to loop context but does not CREATE
    // one — a `break` inside a block that is not inside a loop is still
    // refused.
    assert_eq!(
        emit_unit(&unit_of("{ break }")),
        Err(EmitError::unsupported("break outside loop"))
    );
}

#[test]
fn emits_statement_for_as_a_plain_rust_for() {
    // A `for` in statement position iterates for effects (value
    // discarded) over the SHARED `for_items`, binding the loop var.
    let src = emit_unit(&unit_of(
        "let mut s = 0\nfor x in [1, 2, 3] { s = s + x }\ns",
    ))
    .expect("emit");
    assert!(
        src.contains("for _t_78 in for_items(&(Value::array(vec![Value::Int(1)"),
        "got:\n{src}"
    );
    // The body's value is discarded (statement form) — but a tail-less body
    // is already `Value::Unit`, so NO dead `let _ = Value::Unit;` is emitted
    // when the expression has observable effects.
    assert!(
        !src.contains("let _ = Value::Unit"),
        "dead Unit store: {src}"
    );
}

#[test]
fn a_loop_body_tail_expression_is_still_discarded_for_effects() {
    // Dropping the dead `let _ = Value::Unit;` must not drop a real
    // tail: a body whose tail is an expression still lowers to `let _ = …;`
    // so the tail's side effects run. Here the tail `x + 1` keeps the discard.
    let src = emit_unit(&unit_of(
        "let mut s = 0\nfor x in [1, 2, 3] { s = s + x\nx + 1 }\ns",
    ))
    .expect("emit");
    assert!(
        src.contains("let _ = binary_value(BinaryOp::Add"),
        "a tail expression must still be discarded, not dropped:\n{src}"
    );
    assert!(
        !src.contains("let _ = Value::Unit"),
        "no dead Unit store:\n{src}"
    );
}

#[test]
fn emits_expression_for_collecting_into_an_array() {
    // A `for` in expression position (here the program tail) collects
    // each body value into `Value::array(acc)`.
    let src = emit_unit(&unit_of("for x in [1, 2, 3] { x * 2 }")).expect("emit");
    assert!(src.contains("let mut __acc = Vec::new();"), "got:\n{src}");
    assert!(
        src.contains("__acc.push(binary_value(BinaryOp::Mul, _t_78.clone(), Value::Int(2)"),
        "got:\n{src}"
    );
    assert!(src.contains("Value::array(__acc)"), "got:\n{src}");
}

#[test]
fn for_wildcard_pattern_binds_nothing() {
    let src = emit_unit(&unit_of(
        "let mut s = 0\nfor _ in [1, 2, 3] { s = s + 1 }\ns",
    ))
    .expect("emit");
    assert!(
        src.contains("for _ in for_items(&(Value::array("),
        "got:\n{src}"
    );
}

#[test]
fn break_in_a_statement_for_is_allowed() {
    // A statement `for` may break/continue (§5).
    let src = emit_unit(&unit_of(
        "let mut s = 0\nfor x in [1, 2, 3] { if x == 2 { break }\ns = s + x }\ns",
    ));
    assert!(src.is_ok(), "break in a statement for must emit: {src:?}");
    assert!(src.unwrap().contains("break;"));
}

#[test]
fn break_in_an_expression_for_is_refused() {
    // A value-collecting `for` may NOT break/continue (a §5 static
    // error) — its body is lowered as not-in-loop, so the break is
    // refused.
    assert_eq!(
        emit_unit(&unit_of(
            "for x in [1, 2, 3] { if x == 2 { break } else { x } }"
        )),
        Err(EmitError::unsupported("break outside loop"))
    );
}

#[test]
fn labeled_break_can_cross_an_expression_for() {
    // The expression-`for` itself is not a bare loop-control target, but a
    // labeled break may pass through it to an enclosing labeled `loop`.
    let src = emit_unit(&unit_of(
        "let r = loop 'outer {\n  let xs = for x in [1] {\n    if x == 1 { break 'outer 9 }\n    0\n  }\n  break 0\n}\nr",
    ))
    .expect("emit");
    assert!(src.contains("break 'l0 __brk;"), "got:\n{src}");
}

#[test]
fn labeled_break_cannot_cross_a_function_boundary() {
    assert_eq!(
        emit_unit(&unit_of(
            "loop 'outer {\n  function escape() { break 'outer 9 }\n  break 0\n}"
        )),
        Err(EmitError::unsupported("break to a loop label not in scope"))
    );
}

#[test]
fn value_break_collects_enclosing_function_references() {
    let src = emit_unit(&unit_of(
        "function helper() { 7 }\nfunction choose() { loop { break helper() } }\nchoose()",
    ))
    .expect("a break value must participate in closure capture analysis");
    assert!(src.contains("break 'l0 __brk;"), "got:\n{src}");
    assert!(src.contains("top_cell_get("), "got:\n{src}");
}

#[test]
fn break_in_a_comprehension_body_is_refused() {
    // A comprehension lowers through generated Rust `for` loops, but those are
    // not Topaz loop-control targets.
    assert_eq!(
        emit_unit(&unit_of("[ for x in [1] => { break } ]")),
        Err(EmitError::unsupported("break outside loop"))
    );
}

#[test]
fn the_for_loop_variable_is_immutable() {
    // §5: the loop variable binds immutably; assigning it is refused.
    assert_eq!(
        emit_unit(&unit_of("for x in [1, 2, 3] { x = 9 }")),
        Err(EmitError::unsupported("assign to immutable"))
    );
}

#[test]
fn a_body_let_of_the_loop_variable_is_a_same_scope_redeclaration() {
    // The interpreter runs the body in the same
    // env that binds the loop variable, so a body `let x` (when `x` is
    // the loop var) is a §4 same-scope redeclaration (TPZ5008), NOT a
    // child-scope shadow. The emitter must refuse it.
    assert_eq!(
        emit_unit(&unit_of("for x in [1, 2] { let x = x + 1\nx }")),
        Err(EmitError::unsupported("same-scope redeclaration"))
    );
}

#[test]
fn a_body_let_of_a_different_name_still_shadows_an_outer_binding() {
    // The base fix must not over-refuse: a body `let` of a name that
    // is NOT the loop variable but DOES match an enclosing binding is
    // still legal shadowing.
    let src = emit_unit(&unit_of(
        "let y = 1\nlet mut s = 0\nfor x in [1, 2] { let y = x\ns = s + y }\ns",
    ));
    assert!(
        src.is_ok(),
        "shadowing an outer binding in a for body must emit: {src:?}"
    );
}

#[test]
fn emits_a_typed_for_loop_var() {
    // §6 a scalar typed loop variable conformance-checks each element.
    let src = emit_unit(&unit_of("for x: int in [1, 2] { x }")).expect("emit");
    assert!(
        src.contains("for __loop in") && src.contains("if matches!(&__loop, Value::Int(_))"),
        "got:\n{src}"
    );
}

#[test]
fn a_literal_typed_for_uses_the_literal_guard() {
    let src = emit_unit(&unit_of("for x: 1 in [1] { x }")).expect("emit");
    assert!(
        src.contains("parse::<i64>()")
            && src.contains("let __lit")
            && src.contains("`for` pattern did not match an element"),
        "got:\n{src}"
    );
}
