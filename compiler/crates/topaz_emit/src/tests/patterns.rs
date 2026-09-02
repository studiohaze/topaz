use super::*;

#[test]
fn emits_match_with_literal_cases_and_a_wildcard() {
    // Scrutinee bound once; literal cases compare via values_equal;
    // `_` is the catch-all else.
    let src = emit_unit(&unit_of(
        "match 2 { case 1 => \"one\"\ncase 2 => \"two\"\ncase _ => \"other\" }",
    ))
    .expect("emit");
    assert!(src.contains("let __scrut = Value::Int(2);"), "got:\n{src}");
    assert!(
        src.contains("if values_equal(&(Value::Int(1)), &__scrut)"),
        "got:\n{src}"
    );
    assert!(
        src.contains("else { Value::str(\"other\") }"),
        "got:\n{src}"
    );
}

#[test]
fn emits_match_with_a_binding_catch_all() {
    // A binding catches all and binds the scrutinee (immutably).
    let src = emit_unit(&unit_of("match 5 { case 1 => 10\ncase n => n + 1 }")).expect("emit");
    assert!(src.contains("let _t_6e = __scrut.clone();"), "got:\n{src}");
    assert!(
        src.contains("binary_value(BinaryOp::Add, _t_6e.clone(), Value::Int(1)"),
        "got:\n{src}"
    );
}

#[test]
fn match_without_a_catch_all_emits_the_miss_fault() {
    let src = emit_unit(&unit_of("match 2 { case 1 => 10 }")).expect("emit");
    assert!(
        src.contains("return Err(fault(codes::FAULT_MATCH_MISS,"),
        "got:\n{src}"
    );
}

#[test]
fn emits_a_match_guard() {
    // §5 a guarded binding: the guard sees the binding and lowers to an
    // `if { let n = …; case_guard_bool(…)? }`; a false guard falls through.
    let src = emit_unit(&unit_of(
        "match 2 { case n if n > 1 => \"a\"\ncase _ => \"b\" }",
    ))
    .expect("emit");
    assert!(
        src.contains("case_guard_bool(") && src.contains("let _t_"),
        "got:\n{src}"
    );
}

#[test]
fn a_top_level_return_in_a_guard_is_refused() {
    // §7 a `return` in a `case` GUARD at the top level is still a top-level
    // return (the interpreter faults "return outside a function"), so the
    // bare-return walk must descend into guards and refuse it.
    assert_eq!(
        emit_unit(&unit_of(
            "match 0 { case _ if { return 1\ntrue } => 2\ncase _ => 3 }"
        )),
        Err(EmitError::unsupported("return outside a function"))
    );
}

#[test]
fn emits_a_constructor_pattern() {
    // §6 `case Some(x)` → `if let Value::Some(__inner1) = &__scrut { let x =
    // (**__inner1).clone(); … }`; the binding shadows in the arm scope.
    let src = emit_unit(&unit_of(
        "match Some(5) { case Some(x) => x\ncase None => 0 }",
    ))
    .expect("emit");
    assert!(
        src.contains("if let Value::Some(__inner1) = &__scrut")
            && src.contains("(**__inner1).clone()")
            && src.contains("if let Value::None = &__scrut"),
        "got:\n{src}"
    );
}

#[test]
fn emits_a_guarded_constructor_pattern() {
    // §5/§6 BORROW-SCOPED two-phase guard (`emit_extracted_arm`): the pattern
    // extracts an OWNED tuple (`let __m = if let Value::Some(__inner1) = &__scrut
    // { … Some((..,)) } else { None }`), then the guard runs over the owned binding
    // (`if let Some(..) = __m { if case_guard_bool(..) { Some(..) } else { None } }`)
    // — so no element borrow lives into the guard or body. A false guard → None →
    // the existing `else` chain falls through.
    let src = emit_unit(&unit_of(
        "match Some(5) { case Some(x) if x > 0 => x\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        src.contains("if let Value::Some(__inner1) = &__scrut")
            && src.contains("let __m = if")
            && src.contains("case_guard_bool("),
        "got:\n{src}"
    );
}

#[test]
fn emits_a_nested_constructor_subpattern() {
    // §6 `case Some(Some(x))` nests via a let-chain: an inner `if let
    // Value::Some(__inner2) = &(**__inner1)` then `let x = (**__inner2).clone()`.
    let src = emit_unit(&unit_of(
        "match Some(Some(5)) { case Some(Some(x)) => x\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        src.contains("if let Value::Some(__inner1) = &__scrut")
            && src.contains("let Value::Some(__inner2) = &(**__inner1)")
            && src.contains("(**__inner2).clone()"),
        "got:\n{src}"
    );
}

#[test]
fn emits_a_typed_constructor_subpattern() {
    // §6 `case Some(n: int)` — the inner typed subpattern lowers to a
    // `type_test` condition (here a scalar `matches!`) that binds the name.
    let src = emit_unit(&unit_of(
        "match Some(5) { case Some(n: int) => n\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        src.contains("let Value::Some(__inner") && src.contains("matches!(&(**__inner"),
        "got:\n{src}"
    );
    let literal = emit_unit(&unit_of(
        "match Some(5) { case Some(n: 1) => 1\ncase _ => 0 }",
    ))
    .expect("emit literal typed subpattern");
    assert!(
        literal.contains("parse::<i64>()") && literal.contains("let __lit"),
        "got:\n{literal}"
    );
}

#[test]
fn emits_a_range_pattern() {
    // §6 `case 1..5` (inclusive) → a single `matches!` testing int-and-in-range.
    let src = emit_unit(&unit_of("match 3 { case 1..5 => 1\ncase _ => 0 }")).expect("emit");
    assert!(
        src.contains("matches!(&__scrut, Value::Int(__v) if *__v >= 1 && *__v <= 5)"),
        "got:\n{src}"
    );
    // `..<` is exclusive.
    let ex = emit_unit(&unit_of("match 3 { case 1..<5 => 1\ncase _ => 0 }")).expect("emit");
    assert!(ex.contains("*__v < 5)"), "got:\n{ex}");
    // §6 NEGATIVE endpoints (a `Unary` minus) are evaluated to `i64`
    // (`..` is inclusive).
    let neg = emit_unit(&unit_of("match -8 { case -10..-5 => 1\ncase _ => 0 }")).expect("emit");
    assert!(neg.contains("*__v >= -10 && *__v <= -5)"), "got:\n{neg}");
}

#[test]
fn emits_an_or_pattern() {
    // §6 `case 1 | 2 | 3` → a per-alternative `if … else if … else None` chain,
    // first-match-wins (the v5.4 binding lowering; a non-binding or-pattern just
    // builds empty-tuple blocks). Each alternative is its own `values_equal` test.
    let src = emit_unit(&unit_of("match 1 { case 1 | 2 | 3 => 10\ncase _ => 0 }")).expect("emit");
    assert!(
        src.matches("values_equal(").count() >= 3
            && src.contains("if let Some(()) =")
            && src.contains("} else if "),
        "got:\n{src}"
    );
    // §6 a RANGE alternative in an OR (`case 1 | 5..10`) lowers to a
    // `values_equal` literal test alternative and a `matches!` int-in-range
    // alternative, chained.
    let mixed = emit_unit(&unit_of("match 7 { case 1 | 5..10 => 1\ncase _ => 0 }")).expect("emit");
    assert!(
        mixed.contains("values_equal(")
            && mixed.contains("matches!(&__scrut, Value::Int(__v) if *__v >= 5 && *__v <= 10)")
            && mixed.contains("} else if "),
        "got:\n{mixed}"
    );
    // §6 (v5.4) a BINDING or-pattern `Ok(x) | Err(x)`: each alternative tests its
    // tag, binds `x` from THAT alternative, and the shared tuple `(__x,)` carries
    // the binding into the arm body — first-match-wins.
    let bind = emit_unit(&unit_of(
        "function f(r: Result<int, int>) -> int { match r { case Ok(x) | Err(x) => x } }\nf(Ok(1))",
    ))
    .expect("emit");
    // `x` mangles to `_t_78` (0x78 = 'x'); the shared single-binding tuple is
    // `(_t_78,)`, fed by an `Ok`-tag alternative and an `Err`-tag alternative.
    let xt = mangle("x");
    assert!(
        bind.contains("Value::Ok(")
            && bind.contains("Value::Err(")
            && bind.contains(&format!("({xt},)")),
        "got:\n{bind}"
    );

    // The checker normally rejects duplicate names, but unchecked native
    // emission must refuse them instead of producing an invalid Rust tuple
    // pattern such as `(x, x)`.
    assert_eq!(
        emit_unit(&unit_of(
            "match [1, 2] { case [x, x] | [x, x] => x\ncase _ => 0 }"
        )),
        Err(EmitError::unsupported("same-scope redeclaration"))
    );
}

#[test]
fn emits_a_list_pattern() {
    // §6 `case [a, b]` uses the shared recursive subpattern array binding and
    // one exact slice pattern before extracting owned element bindings.
    let src = emit_unit(&unit_of(
        "match [1, 2] { case [a, b] => a + b\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        src.contains("if let Value::Array(__arr1) = &__scrut")
            && src.contains("let [__aitem2, __aitem3] = &__arr1.borrow()[..]")
            && src.contains("let _t_61 = (*__aitem2).clone()")
            && src.contains("let _t_62 = (*__aitem3).clone()"),
        "got:\n{src}"
    );
    // §6 a NESTED element subpattern `case [Some(x), y]` compiles the element
    // through emit_subpattern over the element reference — a constructor
    // let-chain condition plus binds after the exact slice pattern.
    let nested = emit_unit(&unit_of(
        "let a = [Some(1), 2]\nmatch a { case [Some(x), y] => x + y\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        nested.contains("let [__aitem2, __aitem3] = &__arr1.borrow()[..]")
            && nested.contains("let Value::Some(__inner4) = &(*__aitem2)")
            && nested.contains("let _t_79 = (*__aitem3).clone()"),
        "got:\n{nested}"
    );
}

#[test]
fn emits_a_record_pattern() {
    // §6 `case { x, y }` uses the shared recursive record subpattern route:
    // bind the record once, bind each required field once, then extract owned values.
    let src = emit_unit(&unit_of(
        "let r = { x: 1, y: 2 }\nmatch r { case { x, y } => x + y\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        src.contains("if let Value::Record(__rec1) = &__scrut")
            && src.contains("let Some(__rfield2) = __rec1.get(\"x\")")
            && src.contains("(*__rfield2).clone()"),
        "got:\n{src}"
    );
    // §6 a record field SUBPATTERN `{ s: Some(v) }` compiles the field access
    // the single `__rec1.get("s")` binding through emit_subpattern — a constructor `let`-chain
    // condition plus a `let` bind off the inner value.
    let sub = emit_unit(&unit_of(
        "let r = { s: Some(5) }\nmatch r { case { s: Some(v) } => v\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        sub.contains("let Some(__rfield2) = __rec1.get(\"s\")")
            && sub.contains("Value::Some(__inner")
            && sub.contains("= &(*__rfield2)")
            && sub.contains("**__inner")
            && sub.contains(".clone()"),
        "got:\n{sub}"
    );
}

#[test]
fn emits_a_scalar_typed_pattern() {
    // §6 `case n: int` → a single `Value` variant test that binds the name.
    let src = emit_unit(&unit_of("match 5 { case n: int => n\ncase _ => 0 }")).expect("emit");
    assert!(
        src.contains("if matches!(&__scrut, Value::Int(_))"),
        "got:\n{src}"
    );
    // §6 a UNION of scalar members `case x: int | string` → a `matches!`
    // disjunction binding the name.
    let u = emit_unit(&unit_of(
        "match 5 { case x: int | string => 1\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        u.contains("{ let __u0: &Value = &__scrut; (matches!(__u0, Value::Int(_))) || (matches!(__u0, Value::Str(_))) }"),
        "got:\n{u}"
    );
}

#[test]
fn a_literal_typed_pattern_uses_the_literal_guard_and_falls_through() {
    let src = emit_unit(&unit_of("match 5 { case n: 1 => 1\ncase _ => 0 }")).expect("emit");
    assert!(
        src.contains("parse::<i64>()")
            && src.contains("let __lit")
            && src.contains("else { Value::Int(0) }"),
        "got:\n{src}"
    );
}

#[test]
fn emits_structural_container_typed_patterns() {
    // §6 `Option<T>` / `Result<T, E>` / `Array<T>` now lower to a recursive
    // structural test mirroring the interpreter's `type_matches` Named arms.
    let opt = emit_unit(&unit_of(
        "match Some(5) { case n: Option<int> => 1\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        opt.contains("Value::None => true")
            && opt.contains("Value::Some(__tt0)")
            && opt.contains("matches!(__v0, Value::Int(_))"),
        "got:\n{opt}"
    );
    let res = emit_unit(&unit_of(
        "match Ok(5) { case n: Result<int, string> => 1\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        res.contains("Value::Ok(__tt0)")
            && res.contains("Value::Err(__tt0)")
            && res.contains("matches!(__v0, Value::Str(_))"),
        "got:\n{res}"
    );
    let arr = emit_unit(&unit_of(
        "match [1] { case n: Array<int> => 1\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        arr.contains("Value::Array(__tt0) => __tt0.borrow().iter().all(|__v0|")
            && arr.contains("matches!(__v0, Value::Int(_))"),
        "got:\n{arr}"
    );
    // nested: `Array<Option<int>>` mints distinct counters.
    let nested = emit_unit(&unit_of(
        "match [Some(1)] { case n: Array<Option<int>> => 1\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        nested.contains("Value::Array(__tt0)")
            && nested.contains("Value::Some(__tt1)")
            && nested.contains("matches!(__v1, Value::Int(_))"),
        "got:\n{nested}"
    );
    // §8 `Set<T>` checks every element; a RECORD type checks an exact field
    // set (`len ==`) with each field looked up and recursively checked.
    let set = emit_unit(&unit_of(
        "match Set.of(1) { case n: Set<int> => 1\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        set.contains("Value::Set(__tt0) => __tt0.borrow().items().iter().all(|__v0|")
            && set.contains("matches!(__v0, Value::Int(_))"),
        "got:\n{set}"
    );
    let rec = emit_unit(&unit_of(
        "match { x: 1 } { case n: { x: int } => 1\ncase _ => 0 }",
    ))
    .expect("emit");
    assert!(
        rec.contains("Value::Record(__m0) => __m0.len() == 1")
            && rec.contains("match __m0.get(\"x\") { Some(__v1) => matches!(__v1, Value::Int(_))"),
        "got:\n{rec}"
    );
}

#[test]
fn emits_a_typed_let() {
    // §6 `let x: int = v` wraps the value in a scalar conformance guard
    // (the interpreter's KLetPattern type-match).
    let src = emit_unit(&unit_of("let x: int = 5\nx")).expect("emit");
    assert!(
        src.contains("if matches!(&__v, Value::Int(_))"),
        "got:\n{src}"
    );
}

#[test]
fn a_literal_typed_let_uses_the_literal_guard() {
    let src = emit_unit(&unit_of("let x: 1 = 5\nx")).expect("emit");
    assert!(
        src.contains("parse::<i64>()")
            && src.contains("let __lit")
            && src.contains("`let` pattern did not match the value"),
        "got:\n{src}"
    );
}

#[test]
fn emits_a_function_type_test() {
    // §3 a FUNCTION type lowers to a SHAPE check (`callable_shape_matches`), not
    // a parameter/return inspection — `(int) -> int` is 1 fixed param, non-variadic.
    let src = emit_unit(&unit_of("let f: (int) -> int = (x) => x\nf(1)")).expect("emit");
    assert!(
        src.contains("callable_shape_matches(") && src.contains(", 1, false)"),
        "got:\n{src}"
    );
}

#[test]
fn a_generic_alias_to_a_function_type_emits_a_shape_test() {
    // A generic alias is instantiated first; the function body then lowers to
    // the same SHAPE-only callable check as a direct `(int) -> int` type.
    let src = emit_unit(&unit_of(
        "type FnBox<T> = (T) -> T\nlet f: FnBox<int> = (x) => x\nf(1)",
    ))
    .expect("emit");
    assert!(
        src.contains("callable_shape_matches(") && src.contains(", 1, false)"),
        "got:\n{src}"
    );
}

#[test]
fn an_alias_typed_let_resolves_to_the_body() {
    // §3 a TOP-LEVEL MONOMORPHIC alias is expanded to its body and the body's
    // conformance test is emitted — `type Count = int` lowers to the int
    // guard, exactly as `let x: int = …`. An alias chain resolves too.
    let src = emit_unit(&unit_of("type Count = int\nlet x: Count = 5\nx")).expect("emit");
    assert!(
        src.contains("if matches!(&__v, Value::Int(_))"),
        "got:\n{src}"
    );
    let chained = emit_unit(&unit_of("type A = int\ntype B = A\nlet x: B = 7\nx")).expect("emit");
    assert!(
        chained.contains("if matches!(&__v, Value::Int(_))"),
        "got:\n{chained}"
    );
}

#[test]
fn a_generic_alias_typed_let_resolves_to_the_substituted_body() {
    // `type Box<T> = Array<T>` + `Box<int>` substitutes to `Array<int>` and
    // reuses the existing recursive container test.
    let src =
        emit_unit(&unit_of("type Box<T> = Array<T>\nlet x: Box<int> = [1]\nx")).expect("emit");
    assert!(
        src.contains("Value::Array") && src.contains("Value::Int(_)"),
        "got:\n{src}"
    );
}

#[test]
fn a_selected_generic_nominal_record_typed_let_uses_the_imported_schema() {
    let src = emit_unit(&unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { Box as ForeignBox, makeBox }
function main() -> string {
let b: ForeignBox<int> = makeBox(7)
let c: ForeignBox<string> = ForeignBox { value: "ok" }
"{b.value}:{c.value}"
}
main()
"#,
            ),
            (
                "model.tpz",
                r#"
export record Box<T> { value: T }
export function makeBox(value: int) -> Box<int> {
Box { value: value }
}
"#,
            ),
        ],
    ))
    .expect("emit");
    assert!(
        src.contains(
            "nominal_declaration_identity(record_id.as_ref(), declaration_identity.as_deref()) == \"model::Box\""
        ) && src.contains("Value::Int(_)")
            && src.contains("Value::Str(_)"),
        "got:\n{src}"
    );
}

#[test]
fn a_selected_generic_enum_typed_let_uses_the_imported_schema() {
    let src = emit_unit(&unit_with_files(
        "main.tpz",
        &[
            (
                "main.tpz",
                r#"
import model { Box as ForeignBox }
function main() -> int {
let b: ForeignBox<int> = ForeignBox.One(7)
match b {
    case found: ForeignBox<int> => 1
    case _ => 0
}
}
main()
"#,
            ),
            (
                "model.tpz",
                r#"
export enum Box<T> derives Eq, Order, Show { Empty, One(T), Two(T, T) }
"#,
            ),
        ],
    ))
    .expect("emit");
    assert!(
        src.contains(
            "nominal_declaration_identity(enum_id.as_ref(), declaration_identity.as_deref()) == \"model::Box\""
        )
            && src.contains("\"One\" if payloads.len() == 1")
            && src.contains("Value::Int(_)"),
        "got:\n{src}"
    );
}

#[test]
fn a_recursive_generic_enum_typed_let_uses_a_local_type_helper() {
    let src = emit_unit(&unit_of(
        r#"
enum List<T> derives Eq, Order, Show { Nil, Cons(T, List<T>) }
function main() -> int {
let xs: List<int> = List.Cons(1, List.Cons(2, List.Nil))
match xs {
    case found: List<int> => 1
    case _ => 0
}
}
main()
"#,
    ))
    .expect("emit");
    assert!(
        src.contains("fn __tpz_enum_type_")
            && src.contains("__tpz_enum_type_")
            && src.contains("Value::Int(_)"),
        "recursive generic enum type tests should use a local recursive helper:\n{src}"
    );
}

#[test]
fn a_generic_alias_typed_match_pattern_resolves_to_the_substituted_body() {
    let src = emit_unit(&unit_of(
        "type Alias = int\n\
         type Boxed<T> = Array<T>\n\
         match [1, 2] {\n\
         case ok: Boxed<Alias> => 1\n\
         case _ => 0\n\
         }",
    ))
    .expect("emit");
    assert!(
        src.contains("Value::Array") && src.contains("Value::Int(_)"),
        "got:\n{src}"
    );
}

#[test]
fn a_recursive_alias_is_refused() {
    // A self-recursive alias bounds via the `seen` guard → refuse, never an
    // infinite type-test (the interpreter faults it at runtime).
    assert_eq!(
        emit_unit(&unit_of("type R = R\nlet x: R = 1\nx")),
        Err(EmitError::unsupported("typed let type"))
    );
}

#[test]
fn an_alias_to_a_literal_body_uses_the_literal_type_test() {
    let src = emit_unit(&unit_of("type M = 1\nlet x: M = 1\nx")).expect("emit");
    assert!(
        src.contains("parse::<i64>()") && src.contains("let __lit"),
        "got:\n{src}"
    );
}

#[test]
fn an_alias_poisoned_by_a_block_local_shadow_is_refused() {
    // A top-level `type B = int` is poisoned because a
    // `type B = string` is declared block-locally elsewhere — the
    // interpreter's `lookup_alias_in` is lexical, so resolving the top-level
    // body could be the WRONG one at a shadowed site. The emitter has no
    // per-site lexical scope, so it must REFUSE rather than emit an int guard
    // (which would be an unsound miscompile). The surrounding block emits
    // fine, isolating the poison refusal.
    assert_eq!(
        emit_unit(&unit_of(
            "type B = int\nlet z = { type B = string\n1 }\nlet x: B = 5\nx"
        )),
        Err(EmitError::unsupported("typed let type"))
    );
}

#[test]
fn emits_a_destructuring_let() {
    // §4 `let [a, b] = v` matches one exact slice pattern and binds each element
    // out of a returned tuple; a record `let { x, y } = r` checks the NAMED
    // fields are present (a subset).
    let list = emit_unit(&unit_of("let xs = [1, 2]\nlet [a, b] = xs\na + b")).expect("emit");
    assert!(
        list.contains("let Value::Array(__arr1) = &__dv")
            && list.contains("let [__aitem2, __aitem3] = &__arr1.borrow()[..]")
            && list.contains("let _t_61 = (*__aitem2).clone()")
            && list.contains("let _t_62 = (*__aitem3).clone()"),
        "got:\n{list}"
    );
    let rec = emit_unit(&unit_of("let r = { x: 1, y: 2 }\nlet { x, y } = r\nx + y")).expect("emit");
    assert!(
        rec.contains("let Value::Record(__rec1) = &__dv")
            && rec.contains("let Some(__rfield2) = __rec1.get(\"x\")")
            && rec.contains("let Some(__rfield3) = __rec1.get(\"y\")"),
        "got:\n{rec}"
    );
    // §4 a REST `[head, ..tail]`: one slice pattern binds the prefix and the
    // remaining middle, which `..tail` materializes as an array.
    let rest = emit_unit(&unit_of(
        "let xs = [1, 2, 3]\nlet [head, ..tail] = xs\nhead",
    ))
    .expect("emit");
    assert!(
        rest.contains("let [__aitem2, __arest3 @ ..] = &__arr1.borrow()[..]")
            && rest.contains("Value::array(__arest3.to_vec())"),
        "got:\n{rest}"
    );
    // §4 a NESTED record field with a CONSTRUCTOR subpattern (`{ a: Some(x) }`)
    // routes through `emit_subpattern` in the refutable let-chain form.
    let nested = emit_unit(&unit_of(
        "let r = { a: Some(5) }\nlet { a: Some(x) } = r\nx",
    ))
    .expect("emit");
    assert!(
        nested.contains("if let Value::Record(__rec1) = &__dv")
            && nested.contains("let Some(__rfield2) = __rec1.get(\"a\")")
            && nested.contains("let Value::Some(__inner"),
        "got:\n{nested}"
    );
    // §6/§8 a record field whose subpattern is itself a RECORD now nests
    // (emit_subpattern gained record + list arms): `let Value::Record(__rec…)`.
    let deep =
        emit_unit(&unit_of("let r = { a: { b: 5 } }\nlet { a: { b } } = r\nb")).expect("emit");
    assert!(
        deep.contains("let Value::Record(__rec1) = &__dv")
            && deep.contains("let Some(__rfield4) = __rec3.get(\"b\")"),
        "got:\n{deep}"
    );
    // §6 a REST inside a NESTED list subpattern: the inner array's slice pattern
    // binds its prefix and `..c` middle together.
    let nrest =
        emit_unit(&unit_of("let v = [1, [2, 3, 4]]\nlet [a, [b, ..c]] = v\nb")).expect("emit");
    assert!(
        nrest.contains(
            "let Value::Array(__arr4) = &(*__aitem3) && let [__aitem5, __arest6 @ ..] = &__arr4.borrow()[..]"
        ) && nrest.contains("Value::array(__arest6.to_vec())"),
        "got:\n{nrest}"
    );
    // §4 a NESTED LIST element subpattern (`[Some(x), y]`): the exact slice
    // pattern binds references first, then the constructor consumes its element.
    let nlist = emit_unit(&unit_of(
        "let v = [Some(5), 7]\nlet [Some(x), y] = v\nx + y",
    ))
    .expect("emit");
    assert!(
        nlist.contains(
            "if let Value::Array(__arr1) = &__dv && let [__aitem2, __aitem3] = &__arr1.borrow()[..]"
        ) && nlist.contains("let Value::Some(__inner4) = &(*__aitem2)")
            && nlist.contains("let _t_79 = (*__aitem3).clone()"),
        "got:\n{nlist}"
    );
}

#[test]
fn the_match_binding_is_immutable() {
    // Assigning the bound name in the arm is refused, like a `for`
    // var. (The arm body is a block so the assignment statement is
    // syntactically valid; the binding is still immutable.)
    assert_eq!(
        emit_unit(&unit_of("match 5 { case n => { n = 9\nn } }")),
        Err(EmitError::unsupported("assign to immutable"))
    );
}
