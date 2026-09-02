use super::*;

// ---- rigid member/call projections at non-pattern sites -----------------
// A member access or call on a rigid generic must mint a rigid projection,
// not `Unknown`, so it cannot silently discharge a concrete boundary.

#[test]
fn member_access_on_a_generic_cannot_discharge_a_concrete_return() {
    // `t.field` is `MemberOf<T, field>`, a rigid projection — it must not
    // silently satisfy `-> int`.
    assert_code(
        "function steal<T>(t: T) -> int {\n    return t.field\n}",
        "TPZ5001",
    );
}

#[test]
fn member_call_on_a_generic_cannot_discharge_a_concrete_return() {
    // `t.foo()` is `CallOf<T, foo>` (it bypasses `member_type`, so it needed
    // its own projection).
    assert_code(
        "function steal<T>(t: T) -> int {\n    return t.foo()\n}",
        "TPZ5001",
    );
}

#[test]
fn union_member_access_keeps_the_generic_arm_rigid() {
    // `{ x: int } | T` member `.x` is `int | MemberOf<T, x>`, which can
    // discharge neither `string` nor `int` alone.
    assert_code(
        "function steal<T>(v: { x: int } | T) -> string {\n    return v.x\n}",
        "TPZ5001",
    );
}

#[test]
fn optional_member_on_a_generic_option_preserves_the_projection() {
    // `o?.field` on `Option<T>` is `Option<MemberOf<T, field>>`.
    assert_code(
        "function steal<T>(o: Option<T>) -> Option<int> {\n    return o?.field\n}",
        "TPZ5001",
    );
}

#[test]
fn a_leaked_member_projection_demands_an_annotation() {
    // An omitted return type that would publish the projection is TPZ5022.
    assert_code("function f<T>(t: T) {\n    return t.field\n}", "TPZ5022");
}

#[test]
fn a_locally_unused_member_projection_does_not_over_reject() {
    // The projection is rigid but never escapes a published boundary.
    assert_clean("function f<T>(t: T) {\n    let _x = t.field\n}");
    assert_clean("function id<T>(t: T) -> T {\n    return t\n}");
}

#[test]
fn member_call_on_a_union_with_a_rigid_arm_cannot_discharge_concrete() {
    // The generic arm keeps an opaque `CallOf<T, foo>` in the result, so the
    // precise per-arm projection `string | CallOf<T, foo>` still cannot discharge
    // `int` (the concrete `string` arm is now surfaced, but the rigid arm remains).
    assert_code(
        "function steal<T>(v: { foo: () -> string } | T) -> int {\n    return v.foo()\n}",
        "TPZ5001",
    );
}

#[test]
fn member_call_on_a_union_keeps_each_concrete_arm_result() {
    // The result is the per-arm union (`int | string |
    // CallOf<T, foo>`), so each concrete arm's real return is surfaced while the
    // rigid arm stays opaque — it still cannot discharge a concrete expectation.
    assert_code(
        "type A = { foo: () -> int }\ntype B = { foo: () -> string }\nfunction split<T>(v: A | B | T) -> bool {\n    return v.foo()\n}",
        "TPZ5001",
    );
    // Soundness guard: a concrete `int` arm beside a rigid arm must NOT let the
    // result discharge `int` — the rigid `CallOf<T, foo>` keeps the call opaque.
    // (If this becomes clean, the rigid arm was dropped and the patch is unsound.)
    assert_code(
        "function steal<T>(v: { foo: () -> int } | T) -> int {\n    return v.foo()\n}",
        "TPZ5001",
    );
}

#[test]
fn optional_member_call_on_a_generic_option_projects_not_over_rejects() {
    // `o?.foo()` on `Option<T>` is `Option<CallOf<T, foo>>` — NOT a spurious
    // NOT_CALLABLE on the rigid member projection.
    assert_clean("function f<T>(o: Option<T>) {\n    let _x = o?.foo()\n}");
    assert_code(
        "function f<T>(o: Option<T>) -> Option<int> {\n    return o?.foo()\n}",
        "TPZ5001",
    );
}

#[test]
fn a_non_callable_concrete_union_arm_still_rejects_the_call() {
    // The rigid arm cannot license calling a non-callable concrete member: the
    // `{ foo: int }` arm makes `v.foo()` a real NOT_CALLABLE, projection or not.
    assert_code(
        "function f<T>(v: { foo: int } | T) {\n    let _x = v.foo()\n}",
        "TPZ5005",
    );
    assert_code(
        "function f<T>(o: Option<{ foo: int } | T>) {\n    let _x = o?.foo()\n}",
        "TPZ5005",
    );
}

#[test]
fn a_union_call_checks_args_against_each_concrete_arm() {
    // The receiver may BE the concrete arm at runtime, so that arm's signature
    // is enforced (arity + argument types); the projected RESULT now also carries
    // each concrete arm's real return joined with the rigid `CallOf<...>`.
    assert_code(
        "function f<T>(v: Array<int> | T) {\n    let _x = v.get(\"bad\")\n}",
        "TPZ5001",
    );
    assert_code(
        "function f<T>(v: Array<int> | T) {\n    let _x = v.get()\n}",
        "TPZ5004",
    );
}

#[test]
fn a_union_call_checks_the_pipeline_leading_value_and_mutator_root() {
    // §11: the piped value is checked against the concrete arm's arity even
    // through a rigid-union receiver (foo takes no parameter).
    assert_code(
        "function f<T>(v: { foo: () -> string } | T) -> () {\n    let _x = 1 |> v.foo()\n}",
        "TPZ5004",
    );
    // §9: a mutator that exists on a concrete arm still needs a mutable root.
    assert_code(
        "function f<T>(xs: Array<int> | T) -> () {\n    let _ = xs.push(1)\n}",
        "TPZ5003",
    );
    // §9 also covers merely ACQUIRING the mutator handle on a union arm.
    assert_code(
        "function f<T>(xs: Array<int> | T) -> () {\n    let g = xs.push\n}",
        "TPZ5003",
    );
}

#[test]
fn a_union_call_uses_the_real_method_scheme_for_named_args() {
    // The concrete arm's REAL method scheme is used (not a lossy `Type::Func`),
    // so a bad named arg is rejected exactly as `run` rejects it (check ≡ run).
    assert_code(
        "function f<T>(xs: Array<int> | T) -> () {\n    let _x = xs.get(j: 0)\n}",
        "TPZ5004",
    );
}

#[test]
fn member_closed_arms_are_a_decidable_absence_not_a_projection() {
    // A function (or `unit`) arm exposes NO members, so `.foo` on
    // `(() -> string) | T` is a decidable absence (TPZ5006), not a staged
    // projection that would slip a runtime fault past the checker.
    assert_code(
        "function steal<T>(v: (() -> string) | T) -> int {\n    return v.foo\n}",
        "TPZ5006",
    );
    // The same closure holds for a direct function receiver.
    assert_code("let g = () => 1\nlet x = g.foo", "TPZ5006");
}

// ---- ElemOf<T> at the for-loop and HOF iterable sites -----------------------

#[test]
fn iterating_a_generic_projects_a_rigid_element() {
    // The `for` element of a rigid generic is `ElemOf<T>`, so it cannot silently
    // discharge a concrete expectation.
    assert_code(
        "function f<T>(t: T) -> () {\n    for x in t {\n        let y: int = x\n        print(\"{y}\")\n    }\n}",
        "TPZ5001",
    );
}

#[test]
fn a_hof_callback_element_from_a_generic_is_rigid() {
    // `map`/`filter`/`reduce` over a rigid generic refine the callback element to
    // `ElemOf<T>` (was `Unknown`, which discharged anything).
    assert_code(
        "function f<T>(t: T) -> () {\n    let _ = map(t, (e) => {\n        let y: int = e\n        return y\n    })\n}",
        "TPZ5001",
    );
    // A leaked element projection demands an annotation; ignoring the element is
    // clean (the callback returns a nameable type).
    assert_code(
        "function f<T>(t: T) {\n    return map(t, (e) => e)\n}",
        "TPZ5022",
    );
    assert_clean("function f<T>(t: T) -> Array<int> {\n    return map(t, (e) => 1)\n}");
}

#[test]
fn filter_over_a_generic_does_not_let_ctx_mask_the_element() {
    // `filter`'s input and output element share `Var(0)`, so an expected
    // `Array<int>` return must NOT pre-bind the element away from the rigid
    // `ElemOf<T>` (ordinary + pipeline forms).
    assert_code(
        "function f<T>(t: T) -> Array<int> {\n    return filter(t, (e) => {\n        let y: int = e\n        return true\n    })\n}",
        "TPZ5001",
    );
    assert_code(
        "function f<T>(t: T) -> Array<int> {\n    return t |> filter((e) => {\n        let y: int = e\n        return true\n    })\n}",
        "TPZ5001",
    );
    // A real type-param element (`Map<K, V>.keys` -> `Array<K>`) is rigid too, not
    // just a synthetic projection — `contains_rigid` covers both.
    assert_code(
        "function f<K, V>(m: Map<K, V>) -> Array<int> {\n    return filter(m.keys, (k) => {\n        let y: int = k\n        return true\n    })\n}",
        "TPZ5001",
    );
}

#[test]
fn filter_over_a_concrete_array_does_not_let_ctx_mask_the_element() {
    // `filter`'s input/output share `Var(0)`; a CONCRETE iterable element is
    // ground truth too, so an expected `Array<string>` must NOT mask the real
    // `int` element — `filter(Array<int>)` in that ctx is a mismatch, not masked
    // (regression: the concrete case fell to `unify`, which a ctx pre-binding
    // silently won; rigid was already forced).
    assert_code(
        "let xs: Array<int> = [1]\nlet ys: Array<string> = filter(xs, (x) => true)",
        "TPZ5001",
    );
    // Pipeline form pinned too.
    assert_code(
        "let xs: Array<int> = [1]\nlet ys: Array<string> = xs |> filter((x) => true)",
        "TPZ5001",
    );
}

#[test]
fn sorted_by_generic_identity_rejects_non_orderable_instantiated_key() {
    // `id<T>(x: T) -> T` must instantiate at the callback site. For `Array<P>`,
    // the `sortedBy` key type is the real `P`, whose `Map` field is not ordered.
    assert_code(
        "record P { m: Map<string, int> }\nfunction id<T>(x: T) -> T {\n    return x\n}\nlet xs: Array<P> = [P { m: Map.new() }, P { m: Map.new() }]\nlet ys = xs.sortedBy(id)\nprint(\"{ys.length}\")",
        "TPZ5007",
    );
}

#[test]
fn json_stringify_rejects_generic_foreign_unknown_and_spread_opaque_args() {
    assert_code(
        "function f<T>(x: T) -> Result<string, string> {\n    return JSON.stringify(x)\n}",
        "TPZ5533",
    );
    assert_code(
        "function f(x: Status) -> Result<string, string> {\n    return JSON.stringify(x)\n}",
        "TPZ5533",
    );
    assert_code("let r = JSON.stringify(mystery)\nprint(\"{r}\")", "TPZ5533");
    assert_code(
        "let r = JSON.stringify(...[() => 1])\nprint(\"{r}\")",
        "TPZ5533",
    );
}

#[test]
fn json_stringify_accepts_inference_vars_after_they_resolve_concrete() {
    assert_clean(
        "let xs: Array<int> = []\nlet r: Result<string, string> = JSON.stringify(xs)\nprint(\"{r}\")",
    );
}

#[test]
fn json_protocol_bound_allows_generic_stringify_body() {
    assert_clean(
        "record User derives JSON { name: string }\nfunction encode<T: JSON>(value: T) -> Result<string, string> {\n    return JSON.stringify(value)\n}\nlet r: Result<string, string> = encode(User { name: \"Ada\" })\nprint(\"{r}\")\n0",
    );
}

#[test]
fn json_protocol_bound_rejects_nonconforming_call_site() {
    assert_code(
        "record User { name: string }\nfunction encode<T: JSON>(value: T) -> Result<string, string> {\n    return JSON.stringify(value)\n}\nlet r = encode(User { name: \"Ada\" })\nprint(\"{r}\")\n0",
        "TPZ5522",
    );
}

// ---- array spread element precision and projection -------------------------

#[test]
fn an_array_spread_keeps_concrete_element_precision() {
    // `[...stringArr]` is `Array<string>`, not a poison `Array<Unknown>`, so it
    // cannot satisfy `Array<int>` (the check_expr path) and mixed literals keep
    // their element type.
    assert_code(
        "function f(xs: Array<string>) -> Array<int> {\n    return [...xs]\n}",
        "TPZ5001",
    );
    assert_clean("function f(xs: Array<int>) -> Array<int> {\n    return [0, ...xs, 9]\n}");
}

#[test]
fn an_array_spread_of_a_generic_projects_a_rigid_element() {
    // `[...t]` for a rigid generic is `Array<ElemOf<T>>`, not `Array<Unknown>`.
    assert_code(
        "function f<T>(t: T) -> Array<int> {\n    return [...t]\n}",
        "TPZ5001",
    );
    // The element projection leaks if the result is published unannotated.
    assert_code("function f<T>(t: T) {\n    return [...t]\n}", "TPZ5022");
}

#[test]
fn a_non_array_spread_is_rejected() {
    // Array spread is Array-ONLY; the runtime faults "array spread needs an
    // `Array`" on a scalar or a range, so `check` matches.
    assert_code("function f() -> () {\n    let _ = [...5]\n}", "TPZ5001");
    assert_code("function f() -> () {\n    let _ = [...0..3]\n}", "TPZ5001");
}

#[test]
fn a_union_spread_rejects_non_array_arms_and_keeps_rigid_elements() {
    // A union with a decidably non-array arm (`int`/`null`) faults at runtime, so
    // the spread is rejected.
    assert_code(
        "function f(v: Array<int> | int) -> () {\n    let _ = [...v]\n}",
        "TPZ5001",
    );
    assert_code(
        "function f(v: Array<int> | null) -> () {\n    let _ = [...v]\n}",
        "TPZ5001",
    );
    // A union of arrays where one arm has a rigid element keeps it (the element is
    // `T | int`, not `int`), so it cannot satisfy `Array<int>`.
    assert_code(
        "function f<T>(v: Array<T> | Array<int>) -> Array<int> {\n    return [...v]\n}",
        "TPZ5001",
    );
    // A union of compatible concrete arrays still spreads cleanly.
    assert_clean("function f(v: Array<int> | Array<int>) -> Array<int> {\n    return [...v]\n}");
}

// ---- §4 (v5.4) protocols and derive -------------------------------------
//
// `derives Eq, Order, Show` is CHECKER-ONLY bookkeeping: parse the clause,
// validate name + derivability, record `(protocol, type_id)` conformances. No
// new runtime/codegen — `==`/`<`/render keep working via the value.rs leaves.

/// Parse + check at v5.4 and return the SORTED derive conformance table.
fn conformances(src: &str) -> Vec<(String, String)> {
    let out = parse_with_options(
        FileId(0),
        src,
        ParseOptions {
            language_version: LangVersion::V5_4,
        },
    );
    assert!(out.diagnostics.is_empty(), "test source must parse: {src}");
    check_program_with_version(src, &out.program, LangVersion::V5_4).conformances
}

#[test]
fn derives_on_a_record_record_the_conformance_table() {
    // `record P derives Eq, Order, Show, JSON { a: int, b: int }` parses cleanly
    // and records all four (protocol, type) pairs.
    let src = "record P derives Eq, Order, Show, JSON { a: int, b: int }\n0";
    assert_clean(src);
    assert_eq!(
        conformances(src),
        vec![
            ("Eq".to_string(), "P".to_string()),
            ("JSON".to_string(), "P".to_string()),
            ("Order".to_string(), "P".to_string()),
            ("Show".to_string(), "P".to_string()),
        ],
    );
}

#[test]
fn derives_on_an_enum_record_the_conformance_table() {
    // An enum with a payload variant deriving Eq/Show/JSON — the payload (`string`)
    // is comparable and JSON round-trippable.
    let src = "enum Status derives Eq, Show, JSON { Pending, Done, Failed(string) }\n0";
    assert_clean(src);
    assert_eq!(
        conformances(src),
        vec![
            ("Eq".to_string(), "Status".to_string()),
            ("JSON".to_string(), "Status".to_string()),
            ("Show".to_string(), "Status".to_string()),
        ],
    );
}

#[test]
fn show_is_always_derivable_even_with_a_function_field() {
    // Show needs no comparability, so a function-typed field does not block it.
    let src = "record R derives Show { f: (int) -> int }\n0";
    assert_clean(src);
    assert_eq!(
        conformances(src),
        vec![("Show".to_string(), "R".to_string())],
    );
}

#[test]
fn deriving_eq_on_a_function_typed_field_is_not_derivable() {
    // §4 derivability: Eq requires every field comparable; a function-typed field
    // is non-comparable → TPZ5530, and no Eq conformance is recorded.
    let src = "record R derives Eq { f: (int) -> int }\n0";
    assert_code(src, "TPZ5530");
    assert!(conformances(src).is_empty());
}

#[test]
fn deriving_order_on_a_function_typed_field_is_not_derivable() {
    assert_code("record R derives Order { f: (int) -> int }\n0", "TPZ5530");
}

#[test]
fn deriving_eq_on_a_non_comparable_enum_payload_is_not_derivable() {
    // The same comparability gate over an enum PAYLOAD position.
    assert_code("enum E derives Eq { Wrap((int) -> int) }\n0", "TPZ5530");
}

#[test]
fn deriving_eq_on_a_record_with_enum_nested_map_payload_is_not_derivable() {
    // Derivability must descend through an enum-typed record field. The runtime
    // `Eq.equals` leaf would recurse into `Bad(Map<...>)` and fault; CHECK rejects
    // the derive instead.
    let src =
        "enum Payload { Good(int), Bad(Map<string, int>) }\nrecord R derives Eq { e: Payload }\n0";
    assert_code(src, "TPZ5530");
    assert!(conformances(src).is_empty());
}

#[test]
fn deriving_order_on_a_record_with_enum_nested_function_payload_is_not_derivable() {
    let src =
        "enum Payload { Good(int), Bad((int) -> int) }\nrecord R derives Order { e: Payload }\n0";
    assert_code(src, "TPZ5530");
    assert!(conformances(src).is_empty());
}

#[test]
fn comparable_enum_typed_record_member_can_still_derive() {
    let src = "enum Payload { Empty, Text(string), Count(int) }\nrecord R derives Eq, Order { e: Payload }\n0";
    assert_clean(src);
    assert_eq!(
        conformances(src),
        vec![
            ("Eq".to_string(), "R".to_string()),
            ("Order".to_string(), "R".to_string()),
        ],
    );
}

#[test]
fn an_unknown_derive_name_is_rejected() {
    let src = "record P derives Foo { a: int }\n0";
    assert_code(src, "TPZ5530");
    assert!(conformances(src).is_empty());
}

#[test]
fn deriving_json_rejects_non_json_fields() {
    let src = "record P derives JSON { f: (int) -> int }\n0";
    assert_code(src, "TPZ5530");
    assert!(conformances(src).iter().all(|(p, _)| p != "JSON"),);
}

#[test]
fn deriving_json_rejects_non_json_enum_payloads() {
    let src = "enum E derives JSON { Bad(Result<int, string>) }\n0";
    assert_code(src, "TPZ5530");
    assert!(conformances(src).iter().all(|(p, _)| p != "JSON"),);
}

#[test]
fn a_clean_derive_does_not_disturb_existing_nominal_operations() {
    // Derivation authorizes the capability only — `==` and render keep working on a
    // deriving nominal exactly as on a non-deriving one (no regression).
    assert_clean(
        "record P derives Eq, Order, Show { a: int, b: int }\nlet x = P { a: 1, b: 2 } == P { a: 1, b: 2 }\nprint(\"{x}/{P { a: 1, b: 2 }}\")\n0",
    );
}

#[test]
fn derives_is_a_v54_only_clause_and_stays_an_identifier_at_v53() {
    // The `derives` clause is gated `>= V5_4`. At v5.3 records/enums do not exist
    // either, but `derives` must remain a usable identifier (no clause parsing).
    assert!(
        check_at("let derives = 5\nprint(\"{derives}\")", LangVersion::V5_3).is_empty(),
        "`derives` must stay an ordinary identifier at v5.3",
    );
    // And at v5.4, `derives` outside a record/enum head is still an identifier.
    assert_clean("let derives = 7\nprint(\"{derives}\")");
}

#[test]
fn an_empty_field_record_can_derive_everything() {
    // A zero-field record is trivially comparable, so Eq/Order/Show all derive.
    let src = "record Unit2 derives Eq, Order, Show {}\n0";
    assert_clean(src);
    assert_eq!(conformances(src).len(), 3);
}

// ---- §6 (v5.4) BINDING or-pattern agreement (TPZ5710 / TPZ5711) ---------

#[test]
fn binding_or_pattern_with_agreeing_names_is_clean() {
    // `Ok(x) | Err(x)` over a same-payload Result binds `x` (an `int`) from either
    // alternative — agreeing names AND types, so the arm body sees a single `int`.
    assert_clean(
        "function f(r: Result<int, int>) -> int {\n    match r {\n        case Ok(x) | Err(x) => x + 1\n    }\n}\nf(Ok(1))",
    );
    // A non-binding or-pattern is unchanged — `case 1 | 2 | 3` still checks clean.
    assert_clean(
        "let n: int = 2\nmatch n {\n    case 1 | 2 | 3 => print(\"low\")\n    case _ => print(\"hi\")\n}",
    );
    // An enum binding-or covering payloads of the same type.
    assert_clean(
        "enum Shape { Circle(int), Square(int), Dot }\nfunction side(s: Shape) -> int {\n    match s {\n        case Circle(x) | Square(x) => x\n        case Dot => 0\n    }\n}\nside(Shape.Dot)",
    );
}

#[test]
fn binding_or_pattern_with_mismatched_names_is_tpz5710() {
    // `Ok(x) | Err(y)` binds DIFFERENT names — whichever alternative matched, the
    // other's name would be unbound in the arm body.
    assert_code(
        "function f(r: Result<int, int>) -> int {\n    match r {\n        case Ok(x) | Err(y) => x + 1\n    }\n}\nf(Ok(1))",
        "TPZ5710",
    );
}

#[test]
fn binding_or_pattern_with_mismatched_types_is_tpz5711() {
    // `Ok(x) | Err(x)` over `Result<int, string>` binds `x` at `int` in one
    // alternative and `string` in the other — the types do not unify.
    assert_code(
        "function f(r: Result<int, string>) -> int {\n    match r {\n        case Ok(x) | Err(x) => 0\n    }\n}\nf(Ok(1))",
        "TPZ5711",
    );
}

#[test]
fn binding_or_pattern_type_agreement_is_order_independent() {
    for alternatives in [
        "Some(x: 1) | Some(x: 2) | Some(x: int)",
        "Some(x: int) | Some(x: 1) | Some(x: 2)",
        "Some(x: 2) | Some(x: int) | Some(x: 1)",
    ] {
        assert_clean(&format!(
            "function pick(value: Option<int>) -> int {{\n    match value {{\n        case {alternatives} => x\n        case None => 0\n    }}\n}}\npick(Some(2))"
        ));
    }
    assert_code(
        "function f(r: Result<int, string>) -> int {\n    match r {\n        case Ok(x) | Err(x) => 0\n    }\n}\nf(Ok(1))",
        "TPZ5711",
    );
}

#[test]
fn binding_or_pattern_composes_with_a_guard() {
    // A guarded binding-or arm: the guard sees the bound name; the `false` branch
    // falls through, so the arm contributes no coverage and the catch-all is needed.
    assert_clean(
        "function f(r: Result<int, int>) -> int {\n    match r {\n        case Ok(n) | Err(n) if n > 0 => n\n        case _ => 0\n    }\n}\nf(Ok(1))",
    );
}

#[test]
fn binding_or_pattern_can_make_a_match_exhaustive() {
    // `Ok(v) | Err(v)` covers EVERY Result shape, so no catch-all is needed — the
    // coverage union (after agreement) still exhausts the scrutinee.
    assert_clean(
        "function f(r: Result<int, int>) -> int {\n    match r {\n        case Ok(v) | Err(v) => v\n    }\n}\nf(Ok(1))",
    );
}

// ---- §4 (v5.4) protocol static dispatch -----------------------------------

#[test]
fn protocol_show_on_derived_record_checks_clean_and_types_string() {
    // `Show.show(P{…})` on a `derives Show` record is well-typed and returns string.
    assert_clean(
        "record P derives Show { a: int }\nlet s: string = Show.show(P { a: 1 })\nprint(s)\n0",
    );
}

#[test]
fn protocol_order_compare_on_derived_record_types_int() {
    assert_clean(
        "record P derives Order { a: int }\nlet n: int = Order.compare(P { a: 1 }, P { a: 2 })\nprint(\"{n}\")\n0",
    );
}

#[test]
fn protocol_eq_equals_on_derived_record_types_bool() {
    assert_clean(
        "record P derives Eq { a: int }\nlet b: bool = Eq.equals(P { a: 1 }, P { a: 1 })\nprint(\"{b}\")\n0",
    );
}

#[test]
fn protocol_call_on_a_non_conforming_type_is_tpz5522() {
    // `P` does not `derives Show` and has no `impl Show<P>`, so `Show.show(p)` is a
    // CHECK error (the conformance is required at the call site).
    assert_code(
        "record P { a: int }\nprint(Show.show(P { a: 1 }))\n0",
        "TPZ5522",
    );
}

#[test]
fn protocol_call_with_unknown_method_is_tpz5522() {
    // `Show` has no method `render` — the protocol surface is checked.
    assert_code(
        "record P derives Show { a: int }\nprint(Show.render(P { a: 1 }))\n0",
        "TPZ5522",
    );
}

#[test]
fn manual_impl_show_for_user_checks_clean_and_dispatches() {
    assert_clean(
        "record User { name: string }\nimpl Show<User> { function show(value: User) -> string { value.name } }\nlet s: string = Show.show(User { name: \"Ada\" })\nprint(s)\n0",
    );
}

#[test]
fn user_protocol_with_manual_impl_checks_clean() {
    assert_clean(
        "protocol Greet { function greeting(value: Self) -> string }\nrecord Dog { name: string }\nimpl Greet<Dog> { function greeting(value: Dog) -> string { value.name } }\nlet s: string = Greet.greeting(Dog { name: \"Rex\" })\nprint(s)\n0",
    );
}

#[test]
fn json_protocol_is_builtin_and_derive_only() {
    assert_code(
        "protocol JSON { function encode(value: Self) -> string }\n0",
        "TPZ5008",
    );
    assert_code(
        "record User { name: string }\nimpl JSON<User> { function encode(value: User) -> string { value.name } }\n0",
        "TPZ5022",
    );
}

#[test]
fn orphan_impl_of_builtin_protocol_for_builtin_type_is_tpz5520() {
    // `impl Show<int>` — foreign protocol on a foreign (builtin) type — is an orphan.
    assert_code(
        "impl Show<int> { function show(value: int) -> string { \"x\" } }\n0",
        "TPZ5520",
    );
}

#[test]
fn double_conformance_derive_then_manual_is_tpz5521() {
    // `derives Show` AND `impl Show<User>` both register the conformance — a conflict.
    assert_code(
        "record User derives Show { name: string }\nimpl Show<User> { function show(value: User) -> string { value.name } }\n0",
        "TPZ5521",
    );
}

#[test]
fn two_manual_impls_of_the_same_protocol_is_tpz5521() {
    assert_code(
        "record User { name: string }\nimpl Show<User> { function show(value: User) -> string { value.name } }\nimpl Show<User> { function show(value: User) -> string { \"x\" } }\n0",
        "TPZ5521",
    );
}

#[test]
fn redeclaring_a_builtin_protocol_is_tpz5008() {
    assert_code(
        "protocol Show { function show(value: Self) -> string }\n0",
        "TPZ5008",
    );
}

#[test]
fn protocol_method_call_arg_type_mismatch_is_reported() {
    // `Order.compare(a, b)` requires BOTH args to be the conforming type — mixing a
    // conforming record with an `int` is a type mismatch (TPZ5001) on the 2nd arg.
    assert_code(
        "record P derives Order { a: int }\nprint(\"{Order.compare(P { a: 1 }, 5)}\")\n0",
        "TPZ5001",
    );
}

// ---- §4 (v5.4) protocol generic bounds ------------------------------------

#[test]
fn generic_protocol_bound_allows_static_protocol_call_in_body() {
    assert_clean(
        "record User derives Show { name: string }\nfunction render<T: Show>(value: T) -> string {\n    return Show.show(value)\n}\nlet s: string = render(User { name: \"Ada\" })\nprint(s)\n0",
    );
}

#[test]
fn generic_protocol_bound_rejects_nonconforming_call_site() {
    assert_code(
        "record User { name: string }\nfunction render<T: Show>(value: T) -> string {\n    return Show.show(value)\n}\nlet s = render(User { name: \"Ada\" })\nprint(s)\n0",
        "TPZ5522",
    );
}

#[test]
fn generic_protocol_bound_propagates_through_generic_forwarding() {
    assert_clean(
        "record User derives Show { name: string }\nfunction render<T: Show>(value: T) -> string {\n    return Show.show(value)\n}\nfunction pass<T: Show>(value: T) -> string {\n    return render(value)\n}\nlet s: string = pass(User { name: \"Ada\" })\nprint(s)\n0",
    );
}

#[test]
fn explicit_type_arg_must_satisfy_generic_protocol_bound() {
    assert_clean(
        "record User derives Show { name: string }\nfunction render<T: Show>(value: T) -> string {\n    return Show.show(value)\n}\nlet s: string = render<User>(User { name: \"Ada\" })\nprint(s)\n0",
    );
    assert_code(
        "record User { name: string }\nfunction render<T: Show>(value: T) -> string {\n    return Show.show(value)\n}\nlet s = render<User>(User { name: \"Ada\" })\nprint(s)\n0",
        "TPZ5522",
    );
    assert_clean(
        "record User derives Show { name: string }\nfunction render<T: Show>(value: T) -> string {\n    return Show.show(value)\n}\nfunction pass<T: Show>(value: T) -> string {\n    return render<T>(value)\n}\nlet s: string = pass(User { name: \"Ada\" })\nprint(s)\n0",
    );
}

#[test]
fn bounded_generic_function_as_callback_checks_the_instantiated_bound() {
    assert_clean(
        "record User derives Show { name: string }\nfunction render<T: Show>(value: T) -> string {\n    return Show.show(value)\n}\nlet xs: Array<User> = [User { name: \"Ada\" }]\nlet labels: Array<string> = xs.map(render)\nprint(labels.join(\",\"))\n0",
    );
    assert_code(
        "record User { name: string }\nfunction render<T: Show>(value: T) -> string {\n    return Show.show(value)\n}\nlet xs: Array<User> = [User { name: \"Ada\" }]\nlet labels: Array<string> = xs.map(render)\nprint(labels.join(\",\"))\n0",
        "TPZ5522",
    );
}

#[test]
fn unknown_protocol_bound_is_malformed() {
    assert_code(
        "function f<T: Missing>(value: T) -> T {\n    return value\n}\n0",
        "TPZ5022",
    );
}

#[test]
fn duplicate_protocol_bound_is_tpz5523() {
    assert_code(
        "function f<T: Show + Show>(value: T) -> T { return value }\n0",
        "TPZ5523",
    );
}

#[test]
fn exported_user_protocol_bound_is_tpz5524() {
    assert_code(
        "protocol Label { function label(value: Self) -> string }\n\
         export function labelOf<T: Label>(value: T) -> string { return Label.label(value) }\n0",
        "TPZ5524",
    );
}

#[test]
fn builtin_protocol_bounds_remain_exportable() {
    assert_clean(
        "export function render<T: Show>(value: T) -> string { return Show.show(value) }\n0",
    );
}

#[test]
fn typed_json_rejects_an_open_rigid_schema() {
    assert_code(
        "function decodeMany<T: JSON>(text: string) -> Result<Array<T>, string> {\n\
             return JSON.parseAs<Array<T>>(text)\n\
         }\n0",
        "TPZ5534",
    );
}

#[test]
fn typed_json_accepts_a_closed_local_generic_nominal_schema() {
    assert_clean(
        "record Box<T> { value: T }\n\
         let decoded: Result<Box<int>, string> = JSON.parseAs<Box<int>>(\"null\")\n\
         print(\"{decoded}\")\n0",
    );
    assert_clean(
        "record Box<T> { value: T }\n\
         let decoded: Result<Box<Box<int>>, string> = JSON.parseAs<Box<Box<int>>>(\"null\")\n\
         print(\"{decoded}\")\n0",
    );
    assert_clean(
        "enum Cell<T> { Value(T) }\n\
         let decoded: Result<Cell<int>, string> = JSON.parseAs<Cell<int>>(\"null\")\n\
         print(\"{decoded}\")\n0",
    );
    assert_clean(
        "newtype Id<T> = T\n\
         let decoded: Result<Id<int>, string> = JSON.parseAs<Id<int>>(\"null\")\n\
         print(\"{decoded}\")\n0",
    );
    assert_clean(
        "record Box<T> { value: T }\n\
         record Wrap<T> { inner: Option<Array<Box<T>>> }\n\
         let decoded: Result<Wrap<int>, string> = JSON.parseAs<Wrap<int>>(\"null\")\n\
         print(\"{decoded}\")\n0",
    );
    assert_clean(
        "enum Cell<T> { Value(T) }\n\
         record Envelope<T> { cell: Cell<T> }\n\
         let decoded: Result<Envelope<int>, string> = JSON.parseAs<Envelope<int>>(\"null\")\n\
         print(\"{decoded}\")\n0",
    );
    assert_clean(
        "newtype Id<T> = T\n\
         newtype Wrapped<T> = Id<T>\n\
         let decoded: Result<Wrapped<int>, string> = JSON.parseAs<Wrapped<int>>(\"null\")\n\
         print(\"{decoded}\")\n0",
    );
    assert_clean(
        "record Lookup<K, V> { values: Map<K, V> }\n\
         let decoded: Result<Lookup<string, int>, string> = JSON.parseAs<Lookup<string, int>>(\"null\")\n\
         print(\"{decoded}\")\n0",
    );
}

#[test]
fn typed_json_accepts_root_aliases_and_rejects_block_aliases() {
    assert_clean(
        "type Scalar<T> = T\n\
         type Lookup = Map<string, int>\n\
         record Box<T> { value: Scalar<T>, lookup: Lookup }\n\
         let decoded: Result<Box<int>, string> = JSON.parseAs<Box<int>>(\"null\")\n\
         print(\"{decoded}\")\n0",
    );
    assert_code(
        "let decoded = {\n\
             type Scalar = int\n\
             JSON.parseAs<Scalar>(\"7\")\n\
         }\n\
         print(\"{decoded}\")\n0",
        "TPZ5534",
    );
}

#[test]
fn typed_json_rejects_recursive_and_float_schemas() {
    assert_code(
        "record Node { next: Option<Node> }\nlet n = JSON.parseAs<Node>(\"null\")\n0",
        "TPZ5534",
    );
    assert_code("let n = JSON.parseAs<float>(\"1.0\")\n0", "TPZ5534");
}

#[test]
fn protocol_is_a_v54_only_clause_and_stays_an_identifier_at_v53() {
    // At v5.3 `protocol` is an ordinary identifier — `protocol` as a bare name is just
    // an unbound-identifier program (no protocol decl is parsed).
    let v53 = check_at(
        "let protocol = 1\nprint(\"{protocol}\")\n0",
        LangVersion::V5_3,
    );
    assert!(
        v53.is_empty(),
        "`protocol` must stay an identifier at v5.3, got: {v53:?}"
    );
}
