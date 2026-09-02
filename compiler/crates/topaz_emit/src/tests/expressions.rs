use super::*;

#[test]
fn emits_bool_unit_null() {
    assert!(
        emit_unit(&unit_of("true"))
            .unwrap()
            .contains("let __topaz_init_value = Value::Bool(true);")
    );
    assert!(
        emit_unit(&unit_of("()"))
            .unwrap()
            .contains("let __topaz_init_value = Value::Unit;")
    );
    assert!(
        emit_unit(&unit_of("null"))
            .unwrap()
            .contains("let __topaz_init_value = Value::Null;")
    );
}

#[test]
fn emits_floats_round_trippably() {
    assert!(
        emit_unit(&unit_of("1.5"))
            .unwrap()
            .contains("let __topaz_init_value = Value::Float(1.5);")
    );
    // An integral float keeps its `.0` so it round-trips and never
    // collides with an int literal.
    assert!(
        emit_unit(&unit_of("2.0"))
            .unwrap()
            .contains("let __topaz_init_value = Value::Float(2.0);")
    );
}

#[test]
fn oversized_float_literal_emits_the_infinity_const() {
    // A lexer-valid literal that overflows f64 parses to +inf in
    // BOTH engines; the emitter must emit the const, not the `inf`
    // token `{:?}` produces (which is not a Rust literal and would
    // make the emitted crate fail to compile). Regression for the
    // float/string regression.
    let oversized = format!("2{}.0", "0".repeat(308));
    let src = emit_unit(&unit_of(&oversized)).expect("emit");
    assert!(src.contains("Value::Float(f64::INFINITY)"), "got:\n{src}");
}

#[test]
fn emits_string_literals_with_escapes() {
    assert!(
        emit_unit(&unit_of(r#""hello""#))
            .unwrap()
            .contains(r#"let __topaz_init_value = Value::str("hello");"#)
    );
    // `\n` in the source decodes to a real newline, re-emitted as a
    // Rust `\n` escape — the decoded bytes match the interpreter.
    let src = emit_unit(&unit_of(r#""a\nb""#)).unwrap();
    assert!(src.contains(r#"Value::str("a\nb")"#), "got:\n{src}");
}

#[test]
fn hidden_module_capture_scan_ignores_string_literals() {
    let captures = hidden_rust_module_captures(
        r#"Value::str("__mod_missing"); member_value_required(&__mod_config, "base", SourceSpan::new(0, 1))?"#,
    );
    assert_eq!(captures, vec!["__mod_config".to_string()]);
}

#[test]
fn tagged_templates_lower_to_make_template() {
    // §16 a tagged template builds a `Value::Template` through the shared
    // `make_template` leaf (no longer refused).
    let src = emit_unit(&unit_of(r#"p"x""#)).expect("emit");
    assert!(
        src.contains(r#"make_template("p".to_string()"#),
        "got:\n{src}"
    );
}

#[test]
fn emits_string_interpolation_through_the_shared_render() {
    // `"a{1}b"` builds the value at runtime: text runs are decoded
    // literals, the `{1}` renders through the shared `render`.
    let src = emit_unit(&unit_of(r#""a{1}b""#)).expect("emit");
    assert!(src.contains("let mut __s = String::new();"), "got:\n{src}");
    assert!(src.contains(r#"__s.push_str("a");"#), "got:\n{src}");
    assert!(
        src.contains("__s.push_str(&render(&(Value::Int(1))));"),
        "got:\n{src}"
    );
    assert!(src.contains(r#"__s.push_str("b");"#), "got:\n{src}");
    assert!(src.contains("Value::str(__s)"), "got:\n{src}");
}

#[test]
fn interpolates_a_local_binding() {
    let src = emit_unit(&unit_of("let x = 5\n\"v={x}\"")).expect("emit");
    assert!(
        src.contains("__s.push_str(&render(&(_t_78.clone())));"),
        "got:\n{src}"
    );
}

#[test]
fn emits_binary_arithmetic_as_shared_calls() {
    let src = emit_unit(&unit_of("1 + 2")).expect("emit");
    // Calls the SHARED binary_value with a threaded span, so result
    // and faults are byte-identical to the interpreter.
    assert!(
        src.contains("binary_value(BinaryOp::Add, Value::Int(1), Value::Int(2), Span::new(FileId("),
        "got:\n{src}"
    );
}

#[test]
fn emits_nested_operators_and_unary() {
    // `1 + 2 * 3` nests by precedence; `-(…)` is a unary, paren is
    // transparent.
    let src = emit_unit(&unit_of("-(1 + 2 * 3)")).expect("emit");
    assert!(src.contains("unary_value(UnaryOp::Minus,"), "got:\n{src}");
    assert!(src.contains("binary_value(BinaryOp::Add,"), "got:\n{src}");
    assert!(src.contains("binary_value(BinaryOp::Mul,"), "got:\n{src}");
}

#[test]
fn non_tail_operator_statements_are_evaluated_for_faults() {
    // `1 / 0` is not the tail but must still be emitted (it faults).
    let src = emit_unit(&unit_of("1 / 0\n7")).expect("emit");
    assert!(
        src.contains("let _ = binary_value(BinaryOp::Div,"),
        "got:\n{src}"
    );
    assert!(
        src.contains("let __topaz_init_value = Value::Int(7);"),
        "got:\n{src}"
    );
}

#[test]
fn emits_short_circuit_operators_with_a_lazy_rhs() {
    // §2/§12 `&&`/`||`/`??` lower through the shared `short_circuit_lhs`
    // leaf, with the RHS in the `None` arm so it is evaluated only when
    // the LHS does not short-circuit.
    let and = emit_unit(&unit_of("true && false")).expect("emit");
    assert!(
        and.contains("match short_circuit_lhs(Value::Bool(true), BinaryOp::And,"),
        "got:\n{and}"
    );
    assert!(and.contains("None => Value::Bool(false)"), "got:\n{and}");
    emit_unit(&unit_of("false || true")).expect("emit");
    emit_unit(&unit_of("null ?? 5")).expect("emit");
}

#[test]
fn emits_array_and_record_literals() {
    let arr = emit_unit(&unit_of("[1, 2 + 3]")).unwrap();
    assert!(
        arr.contains("Value::array(vec![Value::Int(1), binary_value(BinaryOp::Add,"),
        "got:\n{arr}"
    );
    let rec = emit_unit(&unit_of("{ x: 1, y: 2 }")).unwrap();
    assert!(rec.contains("Value::record(["), "got:\n{rec}");
    assert!(
        rec.contains("(\"x\".to_string(), Value::Int(1))"),
        "got:\n{rec}"
    );
}

#[test]
fn emits_array_spread_through_the_shared_leaf() {
    // §9 with a spread accumulates at runtime — push regulars, extend
    // spreads via `array_spread_extend`. A no-spread array stays a direct
    // `vec!`.
    let src = emit_unit(&unit_of("let a = [1, 2]\n[0, ...a, 3]")).expect("emit");
    assert!(src.contains("let mut __acc = Vec::new();"), "got:\n{src}");
    assert!(src.contains("__acc.push(Value::Int(0));"), "got:\n{src}");
    assert!(
        src.contains("array_spread_extend(&mut __acc, _t_61.clone(),"),
        "got:\n{src}"
    );
    // No spread → no accumulator.
    assert!(
        emit_unit(&unit_of("[1, 2]"))
            .unwrap()
            .contains("Value::array(vec![Value::Int(1), Value::Int(2)])")
    );
}

// A spread operand is lowered, so the capture walkers must
// descend into it — else a name used only in a spread is missed (an
// over-refusal) and a closure under a spread escapes the later-shadow
// guard (a soundness divergence).

#[test]
fn a_capture_used_only_in_a_spread_emits() {
    emit_unit(&unit_of("let a = [1]\nlet f = () => [...a]\nf()")).expect("emit");
}

#[test]
fn a_closure_under_a_spread_shadowed_later_is_refused() {
    assert_eq!(
        emit_unit(&unit_of(
            "let x = 1\n{ let fs = [...[() => x]]\nlet x = 2\nfs[0]() }"
        )),
        Err(EmitError::unsupported(
            "declaration shadows a captured binding"
        ))
    );
}

// The capture walkers also descend into member access and
// indexing (both now emit) — else a closure under `[..][i]` or `.f`
// escapes the later-shadow guard (soundness), and a capture used only
// through `a[i]`/`r.f` is over-refused.

#[test]
fn a_closure_under_an_index_shadowed_later_is_refused() {
    assert_eq!(
        emit_unit(&unit_of(
            "let x = 1\n{ let f = [() => x][0]\nlet x = 2\nf() }"
        )),
        Err(EmitError::unsupported(
            "declaration shadows a captured binding"
        ))
    );
}

#[test]
fn a_closure_under_a_member_shadowed_later_is_refused() {
    assert_eq!(
        emit_unit(&unit_of(
            "let x = 1\n{ let f = { f: () => x }.f\nlet x = 2\nf() }"
        )),
        Err(EmitError::unsupported(
            "declaration shadows a captured binding"
        ))
    );
}

#[test]
fn a_capture_used_only_through_index_or_member_emits() {
    emit_unit(&unit_of("let a = [1]\nlet f = () => a[0]\nf()")).expect("emit");
    emit_unit(&unit_of("let r = { n: 5 }\nlet f = () => r.n\nf()")).expect("emit");
}

// R3 fold: an INTERPOLATED string pattern literal is parser-accepted but
// the interpreter FAULTS on it at runtime (TPZ5001 — interpolation is not
// allowed in pattern literals), so `emit_match` refuses it rather than emit
// a program that diverges by completing. (This keeps a pattern literal a
// plain constant, so the capture walkers need not descend into patterns.)

#[test]
fn an_interpolated_pattern_literal_is_unsupported() {
    assert_eq!(
        emit_unit(&unit_of(
            "let a = \"hi\"\nmatch a { case \"${a}\" => 1\ncase _ => 0 }"
        )),
        Err(EmitError::unsupported("interpolation in pattern literal"))
    );
}

#[test]
fn emits_let_binding_and_ident_read() {
    // `x` mangles to `_t_78` (hex of its UTF-8 bytes) — an injective,
    // Rust-keyword-free, ASCII-only local.
    let src = emit_unit(&unit_of("let x = 5\nx")).unwrap();
    assert!(src.contains("let _t_78 = Value::Int(5);"), "got:\n{src}");
    assert!(
        src.contains("let __topaz_init_value = _t_78.clone();"),
        "got:\n{src}"
    );
}

#[test]
fn emits_mutable_rebinding() {
    let src = emit_unit(&unit_of("let mut x = 1\nx = 2\nx")).unwrap();
    assert!(
        src.contains("let mut _t_78 = Value::Int(1);"),
        "got:\n{src}"
    );
    assert!(src.contains("_t_78 = Value::Int(2);"), "got:\n{src}");
}

#[test]
fn unicode_identifiers_mangle_to_valid_rust() {
    // An emoji identifier is a valid Topaz name but not a valid Rust
    // one — it must mangle, never emit raw. Regression for bindings.
    let src = emit_unit(&unit_of("let 😀 = 1\n😀")).unwrap();
    assert!(
        !src.contains('😀'),
        "raw unicode leaked into emitted Rust:\n{src}"
    );
    assert!(src.contains("_t_f09f9880"), "got:\n{src}");
}

#[test]
fn free_identifier_is_unsupported() {
    // A name that is not a local, not the prelude `None`, and not a free
    // builtin (every free builtin — toInt/print/open/map/filter/reduce — is
    // now a first-class value) is a genuinely UNBOUND name, which the
    // interpreter runtime-faults `GUARD_UNBOUND`; the emitter declines it.
    assert_eq!(
        emit_unit(&unit_of("undefined_name")),
        Err(EmitError::unsupported("free identifier"))
    );
}

#[test]
fn emits_compound_assignment_reading_the_target_first() {
    // §2 `x += e` → `x = binary_value(Add, x.clone(), e, span)?` — the
    // target read (`x.clone()`) is the FIRST argument, so it evaluates
    // before the RHS.
    let src = emit_unit(&unit_of("let mut x = 1\nx += 2")).expect("emit");
    assert!(
        src.contains("_t_78 = binary_value(BinaryOp::Add, _t_78.clone(), Value::Int(2),"),
        "got:\n{src}"
    );
    emit_unit(&unit_of("let mut x = 10\nx -= 1\nx *= 2\nx /= 3\nx %= 2")).expect("emit");
}

#[test]
fn emits_a_coalescing_assignment_with_a_lazy_rhs() {
    // §12 `x ??= e` writes only when x is null/None; the RHS sits inside
    // the branch so it runs only then.
    let src = emit_unit(&unit_of("let mut x = null\nx ??= 1")).expect("emit");
    assert!(
        src.contains("if matches!(&_t_78, Value::Null | Value::None) { _t_78 = Value::Int(1); }"),
        "got:\n{src}"
    );
}

#[test]
fn same_scope_redeclaration_is_refused() {
    // §4 redeclaration is a static error / runtime GUARD_REDECLARE —
    // the emitter refuses rather than silently Rust-shadowing.
    assert_eq!(
        emit_unit(&unit_of("let x = 1\nlet x = 2\nx")),
        Err(EmitError::unsupported("same-scope redeclaration"))
    );
}

#[test]
fn assignment_to_immutable_is_refused() {
    // Assigning a non-`mut` local is a static error / GUARD_IMMUTABLE
    // — refuse rather than emit `x = …` on an immutable Rust local.
    assert_eq!(
        emit_unit(&unit_of("let x = 1\nx = 2\nx")),
        Err(EmitError::unsupported("assign to immutable"))
    );
}

#[test]
fn the_tail_statement_is_the_value() {
    // Earlier pure literals drop out; the last is the result.
    let src = emit_unit(&unit_of("1\n2\n3")).expect("emit");
    assert!(
        src.contains("let __topaz_init_value = Value::Int(3);"),
        "got:\n{src}"
    );
}

#[test]
fn emits_a_direct_lambda_call_over_the_callable_abi() {
    // `((x) => x + 1)(5)` — the FIRST exercise of the async TpzCall
    // ABI: the lambda lowers to a `Value::Closure` over an
    // `EmittedClosure`, and the call goes through `call_value(...)
    // .await`.
    let src = emit_unit(&unit_of("((x) => x + 1)(5)")).expect("emit");
    assert!(
        src.contains("Value::Closure(Rc::new(EmittedClosure {"),
        "got:\n{src}"
    );
    assert!(
        src.contains("let _t_78 = __args.next().expect("),
        "got:\n{src}"
    );
    assert!(src.contains("call_value("), "got:\n{src}");
    assert!(src.contains(".await?"), "got:\n{src}");
}

#[test]
fn emits_a_lambda_capturing_an_immutable_binding() {
    // `(y) => x + y` captures the enclosing immutable `x`: snapshot
    // clone outside, owned by the `move` closure, re-cloned per call.
    let src = emit_unit(&unit_of("let x = 1\n((y) => x + y)(2)")).expect("emit");
    assert!(
        src.contains("let __cap_t_78 = _t_78.clone();"),
        "got:\n{src}"
    );
    assert!(src.contains("EmittedClosure { call: move |"), "got:\n{src}");
}

#[test]
fn a_mutable_capture_becomes_a_cell() {
    // §5 capturing a `let mut` makes it a rebinding cell (a shared
    // `Rc<RefCell<Value>>`), read through `cell_get` — so a later mutation
    // is visible inside the closure (the interpreter's live-env capture).
    let src = emit_unit(&unit_of("let mut a = 1\n((x) => x + a)(2)")).expect("emit");
    assert!(
        src.contains("cell_new(Value::Int(1))") && src.contains("cell_get(&"),
        "got:\n{src}"
    );
}

#[test]
fn an_inner_mutable_capture_is_also_a_cell() {
    // §5 a captured `let mut` in a NESTED block is a cell too (the
    // escape analysis runs per scope). The INNERMOST binding decides:
    // here the lambda captures the inner `let mut a`, which is the cell.
    let src = emit_unit(&unit_of("let a = 1\n{ let mut a = 2\n((x) => x + a)(5) }")).expect("emit");
    assert!(src.contains("cell_new(Value::Int(2))"), "got:\n{src}");
}

#[test]
fn an_uncaptured_mutable_stays_a_plain_let_mut() {
    // §5 a `let mut` that NO closure captures keeps its plain Rust `let
    // mut` (no cell overhead, nothing observes it through a closure).
    let src = emit_unit(&unit_of("let mut a = 1\na = 2\na")).expect("emit");
    assert!(
        src.contains("let mut _t_") && !src.contains("cell_new"),
        "got:\n{src}"
    );
}

#[test]
fn a_lambda_that_does_not_reference_a_shadowing_local_still_emits() {
    // `print` is an enclosing local but `(x) => x` does not reference
    // it, so nothing is captured and the lambda emits normally.
    let src = emit_unit(&unit_of("let print = 1\n((x) => x)(2)")).expect("emit");
    assert!(src.contains("EmittedClosure {"), "got:\n{src}");
}

#[test]
fn emits_collection_constructors() {
    assert!(
        emit_unit(&unit_of("Array.of(1, 2)"))
            .unwrap()
            .contains("Value::array(vec![Value::Int(1), Value::Int(2)])")
    );
    assert!(
        emit_unit(&unit_of("Set.of(1, 2)"))
            .unwrap()
            .contains("builtin_set_of(vec![Value::Int(1), Value::Int(2)],")
    );
    assert!(
        emit_unit(&unit_of("Map.new()"))
            .unwrap()
            .contains("builtin_map_new()")
    );
    assert!(
        emit_unit(&unit_of("Map.ofEntries([{ key: \"a\", value: 1 }])"))
            .unwrap()
            .contains("builtin_map_of_entries(")
    );
    let fixed_spread = emit_unit(&unit_of("JSON.parse(...[\"null\"])\nMap.new(...[])"))
        .expect("fixed namespace spread faults should emit");
    assert!(
        fixed_spread.contains("call_spread_extend(&mut __tpz_ns_spread"),
        "got:\n{fixed_spread}"
    );
    assert!(
        fixed_spread.contains("spread arguments require a variadic parameter"),
        "got:\n{fixed_spread}"
    );
}

#[test]
fn map_new_with_arguments_is_unsupported() {
    assert_eq!(
        emit_unit(&unit_of("Map.new(1)")),
        Err(EmitError::unsupported("Map.new takes no arguments"))
    );
}

#[test]
fn a_shadowed_constructor_head_is_not_the_constructor() {
    // `Array` shadowed by a local is a member access on that local
    // (`.of` on an `Int`), NOT the `Array.of` constructor — so it lowers
    // to the shared `member_value_required` leaf, which faults at runtime
    // exactly as the interpreter does (no-member on `Int`), rather than
    // emitting the constructor.
    let src = emit_unit(&unit_of("let Array = 1\nArray.of(2)")).expect("emit");
    assert!(
        src.contains("member_value_required(&(_t_4172726179.clone()), \"of\","),
        "got:\n{src}"
    );
    assert!(
        !src.contains("Value::array(vec![Value::Int(2)])"),
        "got:\n{src}"
    );
}
