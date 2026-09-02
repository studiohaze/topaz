use super::*;

// ---- §13 `?` -----------------------------------------------------------

#[test]
fn try_unwraps_the_result_value() {
    assert_clean(
        "function parse(s: string) -> Result<int, string> {\n    return Ok(1)\n}\nfunction run() -> Result<int, string> {\n    let n: int = parse(\"1\")?\n    return Ok(n + 1)\n}",
    );
}

#[test]
fn try_error_must_match_the_declared_return() {
    assert_code(
        "function parse(s: string) -> Result<int, int> {\n    return Ok(1)\n}\nfunction run() -> Result<int, string> {\n    let n: int = parse(\"1\")?\n    return Ok(n)\n}",
        "TPZ5001",
    );
}

#[test]
fn try_in_a_non_result_function_is_tpz5001() {
    assert_code(
        "function parse(s: string) -> Result<int, string> {\n    return Ok(1)\n}\nfunction run() -> int {\n    let n: int = parse(\"1\")?\n    return n\n}",
        "TPZ5001",
    );
}

#[test]
fn try_on_a_non_result_value_is_tpz5001() {
    assert_code(
        "function run() -> Result<int, string> {\n    let n: int = 1\n    let m: int = n?\n    return Ok(m)\n}",
        "TPZ5001",
    );
}

// ---- §12 `??` ----------------------------------------------------------

#[test]
fn coalesce_unwraps_one_optional_layer() {
    assert_clean("let xs: Array<int> = [1, 2]\nlet n: int = xs.get(0) ?? 0");
    assert_clean("let v: string | null = null\nlet s: string = v ?? \"fallback\"");
}

#[test]
fn coalesce_fallback_must_match_the_inner_type() {
    assert_code(
        "let xs: Array<int> = [1, 2]\nlet n = xs.get(0) ?? \"zero\"\nprint(\"{n}\")",
        "TPZ5001",
    );
}

#[test]
fn coalesce_on_a_concrete_non_optional_is_tpz5001() {
    assert_code("let n: int = 1\nlet m = n ?? 0\nprint(\"{m}\")", "TPZ5001");
}

#[test]
fn coalesce_on_ambient_values_stays_silent() {
    assert_clean("let user = loadUser(42)\nlet name = user ?? \"guest\"\nprint(\"{name}\")");
}

// ---- §12 `?.` ----------------------------------------------------------

#[test]
fn optional_access_rewraps_in_the_container() {
    assert_clean("let u: Option<{ name: string }> = None\nlet n: Option<string> = u?.name");
    assert_clean("let u: { name: string } | null = null\nlet n: string | null = u?.name");
}

#[test]
fn optional_call_flat_maps_the_method_result() {
    // §12 lowering is map/flatMap: an optional member result does
    // not double-wrap.
    assert_clean("let xs: Option<Array<int>> = Some([1, 2])\nlet n: Option<int> = xs?.get(0)");
}

#[test]
fn chained_optional_access_flat_maps() {
    // The §12 worked example: each hop yields Option<...>, never
    // Option<Option<...>>.
    assert_clean(
        "let user: Option<{ name: string, profile: Option<{ city: string }> }> =\n    Some({ name: \"Ann\", profile: Some({ city: \"Seoul\" }) })\nlet city: Option<string> = user?.profile?.city\nlet display: string = user?.profile?.city ?? \"Unknown\"",
    );
}

#[test]
fn optional_access_member_errors_surface() {
    assert_code(
        "let u: Option<{ name: string }> = None\nlet n = u?.email\nprint(\"{n}\")",
        "TPZ5006",
    );
}

#[test]
fn optional_access_on_a_concrete_non_optional_is_tpz5001() {
    assert_code(
        "let u: { name: string } = { name: \"t\" }\nlet n = u?.name\nprint(\"{n}\")",
        "TPZ5001",
    );
}

// ---- §12 `??=` ---------------------------------------------------------

#[test]
fn coalesce_assign_takes_the_full_target_type() {
    // §12: no implicit Some(value) wrapping — the value must be
    // assignable to the target type itself.
    assert_clean("let mut v: Option<int> = None\nv ??= Some(3)");
    assert_clean("let mut s: string | null = null\ns ??= \"guest\"");
    assert_code("let mut v: Option<int> = None\nv ??= 3", "TPZ5001");
    assert_code(
        "let mut v: Option<int> = None\nv ??= Some(\"three\")",
        "TPZ5001",
    );
    assert_code("let mut n: int = 1\nn ??= 2", "TPZ5001");
}

// ---- review fold (r1) ------------------------------------------------------

#[test]
fn coalesce_fallback_is_a_context_site() {
    // The reviewer's counterexample: contextual literals must
    // survive into the RHS instead of widening to string.
    assert_clean(
        "type Mode = \"on\" | \"off\"\nlet m: Option<Mode> = None\nlet out: Mode = m ?? \"on\"\nprint(out)",
    );
    assert_clean("let xs: Option<Array<int>> = None\nlet ys: Array<int> = xs ?? []");
}

#[test]
fn type_patterns_bind_the_narrowed_overlap() {
    // `x` is int here, not int | string: the binding takes the
    // overlap of scrutinee and annotation.
    assert_clean("let n: int = 1\nmatch n {\n    case x: int | string => print(\"{x + 1}\")\n}");
}

#[test]
fn impossible_container_type_patterns_are_tpz5001() {
    assert_code(
        "let o: Option<int> = None\nmatch o {\n    case r: Result<int, string> => print(\"no\")\n    case _ => print(\"ok\")\n}",
        "TPZ5001",
    );
}

#[test]
fn impossible_record_and_list_patterns_are_tpz5001() {
    assert_code(
        "let n: int = 1\nmatch n {\n    case { value } => print(\"{value}\")\n    case _ => print(\"ok\")\n}",
        "TPZ5001",
    );
    assert_code(
        "let n: int = 1\nmatch n {\n    case [x] => print(\"{x}\")\n    case _ => print(\"ok\")\n}",
        "TPZ5001",
    );
}

#[test]
fn record_and_list_patterns_match_a_union_member() {
    // A record pattern matches any record MEMBER of a union scrutinee — the
    // canonical "match a discriminated union by shape" idiom. Previously this
    // was over-rejected as "can never match" (the arm only accepted a bare
    // Record). Each matching member's field type flows to the binding.
    assert_clean(
        "type A = { x: int }\ntype B = { y: int }\nfunction f(v: A | B) -> int {\n    return match v {\n        case { x: n } => n + 1\n        case { y: n } => n\n        case _ => 0\n    }\n}",
    );
    // A list pattern likewise matches an array member of a union.
    assert_clean(
        "type Xs = Array<int>\ntype Ys = Array<string>\nfunction g(v: Xs | Ys) -> int {\n    return match v {\n        case [a] => 1\n        case _ => 0\n    }\n}",
    );
}

#[test]
fn record_or_list_pattern_on_a_union_without_that_member_still_rejects() {
    // The fix stays sound: a union with NO record member still rejects a record
    // pattern as impossible (and likewise no array member rejects a list one).
    assert_code(
        "let v: int | string = 1\nmatch v {\n    case { x: n } => print(\"{n}\")\n    case _ => print(\"ok\")\n}",
        "TPZ5001",
    );
    assert_code(
        "let v: int | string = 1\nmatch v {\n    case [a] => print(\"{a}\")\n    case _ => print(\"ok\")\n}",
        "TPZ5001",
    );
}

#[test]
fn unit_typed_pattern_only_rejects_when_the_scrutinee_cannot_match_unit() {
    assert_code_and_message(
        "function f() -> int {\n    return match 7 {\n        case n: () => 1\n        case _ => 0\n    }\n}",
        "TPZ5001",
        "this pattern can never match",
    );
    assert_clean(
        "function f() -> int {\n    return match () {\n        case n: () => 1\n        case _ => 0\n    }\n}",
    );
}

#[test]
fn tagged_union_pattern_narrows_sibling_fields_by_discriminant() {
    // A literal discriminant narrows the union to the matched variant, so a
    // sibling binding keeps that variant's field type (not the union of all):
    // `n + 1` requires `x` to be `int`, which holds only because `kind: "a"`
    // narrowed to member A.
    assert_clean(
        "type A = { kind: \"a\", x: int }\ntype B = { kind: \"b\", x: string }\nfunction f(v: A | B) -> int {\n    return match v {\n        case { kind: \"a\", x: n } => n + 1\n        case { kind: \"b\", x: s } => 0\n        case _ => 0\n    }\n}",
    );
    // A discriminant no member can satisfy is impossible.
    assert_code(
        "type A = { kind: \"a\", ok: false }\ntype B = { kind: \"b\", ok: true }\nfunction f(v: A | B) -> int {\n    return match v {\n        case { kind: \"a\", ok: true } => 1\n        case _ => 0\n    }\n}",
        "TPZ5001",
    );
}

#[test]
fn record_literal_narrows_to_a_union_member() {
    // A record literal checked against a union of records narrows to the member
    // whose field set (and literal discriminant) it matches, so a literal-typed
    // field (`kind: "circle"`) stays literal instead of widening to `string`.
    assert_clean(
        "type Circle = { kind: \"circle\", radius: float }\ntype Rect = { kind: \"rect\", w: float, h: float }\nlet c: Circle | Rect = { kind: \"circle\", radius: 2.0 }\nc",
    );
    // A literal matching no member is still rejected.
    assert_code(
        "type Circle = { kind: \"circle\", radius: float }\ntype Rect = { kind: \"rect\", w: float, h: float }\nlet c: Circle | Rect = { kind: \"square\", radius: 1.0 }\nc",
        "TPZ5001",
    );
    // Disambiguation also works on a NON-literal field whose type differs across
    // members: `x: "ok"` is assignable only to the `x: string` member.
    assert_clean(
        "type A = { kind: \"x\", x: int }\ntype B = { kind: \"x\", x: string }\nlet v: A | B = { kind: \"x\", x: \"ok\" }\nv",
    );
}

#[test]
fn generic_union_member_projects_a_rigid_field_not_unknown() {
    // A generic/opaque union member is RIGID, not gradual: destructuring its
    // field yields a fresh rigid projection `FieldOf<T, x>`, which can neither
    // discharge a concrete type NOR be used as `T`. Extracting the field at a
    // concrete type is therefore correctly REJECTED — closing the silent-
    // corruption hole where the binding previously leaked through `Unknown` and
    // discharged any boundary. Proven by it being rejected as BOTH `int` and
    // `string`.
    assert_code(
        "function f<T>(v: { x: int } | T) -> int {\n    return match v {\n        case { x } => x\n        case _ => 0\n    }\n}",
        "TPZ5001",
    );
    assert_code(
        "function f<T>(v: { x: int } | T) -> string {\n    return match v {\n        case { x } => x\n        case _ => \"z\"\n    }\n}",
        "TPZ5001",
    );
    // The counterexample over a PURE rigid `T`: the field is not `T` itself, so
    // returning it as `T` is rejected.
    assert_code(
        "function bad<T>(v: T) -> T {\n    return match v {\n        case { x } => x\n        case _ => v\n    }\n}",
        "TPZ5001",
    );
    // The pattern itself stays ALLOWED — only USING the rigid binding at a
    // concrete type is rejected. An unused projection is fine.
    assert_clean(
        "function ok<T>(v: T) -> bool {\n    return match v {\n        case { x } => true\n        case _ => false\n    }\n}",
    );
}

#[test]
fn a_leaked_projection_forces_an_explicit_return_annotation() {
    // An OMITTED return type that would publish a rigid projection (an unnameable
    // `FieldOf<T, x>`) is rejected with a request to annotate (TPZ5022), rather
    // than leaking the projection into the inferred signature.
    assert_code(
        "function f<T>(v: T) {\n    return match v {\n        case { x } => x\n        case _ => v\n    }\n}",
        "TPZ5022",
    );
    // The same body WITH a concrete annotation is checked against it — and the
    // projection cannot discharge it (TPZ5001).
    assert_code(
        "function f<T>(v: T) -> int {\n    return match v {\n        case { x } => x\n        case _ => 0\n    }\n}",
        "TPZ5001",
    );
}

#[test]
fn a_generic_list_element_projects_a_rigid_elemof_not_unknown() {
    // The list analogue of the record hole: a generic/opaque list member yields a
    // rigid `ElemOf<T>` element projection (and an `Array<ElemOf<T>>` rest), so
    // extracting the element at a concrete type is REJECTED.
    assert_code(
        "function g<T>(v: Array<int> | T) -> string {\n    return match v {\n        case [a] => a\n        case _ => \"z\"\n    }\n}",
        "TPZ5001",
    );
    // The rest binds `Array<int> | Array<ElemOf<T>>`, which cannot discharge a
    // concrete `Array<int>` either.
    assert_code(
        "function r<T>(v: Array<int> | T) -> Array<int> {\n    return match v {\n        case [a, ..rest] => rest\n        case _ => []\n    }\n}",
        "TPZ5001",
    );
    // An unused element/rest projection is fine.
    assert_clean(
        "function ok<T>(v: T) -> bool {\n    return match v {\n        case [a, ..rest] => true\n        case _ => false\n    }\n}",
    );
}

#[test]
fn a_generic_constructor_payload_projects_a_rigid_payloadof_not_unknown() {
    // `case Some(x)` over a rigid `T` binds the payload to `PayloadOf<T, Some>`,
    // which cannot discharge a concrete type — extracting it as `int` is REJECTED.
    assert_code(
        "function h<T>(v: T) -> int {\n    return match v {\n        case Some(x) => x\n        case _ => 0\n    }\n}",
        "TPZ5001",
    );
    // An unused payload projection is fine.
    assert_clean(
        "function ok<T>(v: T) -> bool {\n    return match v {\n        case Some(x) => true\n        case _ => false\n    }\n}",
    );
}

#[test]
fn a_constructor_pattern_splits_a_union_with_a_rigid_member() {
    // `Option<int> | T` with `case Some(x)`: the `Option` member definitely
    // matches (payload `int`) and the rigid `T` member might (payload
    // `PayloadOf<T, Some>`), so the pattern is ACCEPTED (not "can never match")
    // and binds `int | PayloadOf<T, Some>` — like the record/list union split.
    assert_clean(
        "function ok<T>(v: Option<int> | T) -> bool {\n    return match v {\n        case Some(x) => true\n        case _ => false\n    }\n}",
    );
    assert_clean(
        "function ok<T>(v: Result<int, string> | T) -> bool {\n    return match v {\n        case Ok(x) => true\n        case Err(e) => false\n        case _ => false\n    }\n}",
    );
    // But using that payload at a concrete type is still REJECTED: the rigid
    // member's `PayloadOf<T, Some>` cannot discharge `int`.
    assert_code(
        "function bad<T>(v: Option<int> | T) -> int {\n    return match v {\n        case Some(x) => x\n        case _ => 0\n    }\n}",
        "TPZ5001",
    );
}

#[test]
fn a_known_constructor_pattern_enforces_its_arity() {
    // Some/Ok/Err take exactly one subpattern; None takes none. A wrong arity is
    // rejected at CHECK (TPZ5004) — including through a rigid/gradual union member
    // that does not itself re-check arity — matching the interpreter's runtime
    // guard so check and run agree (no silent corruption).
    assert_code(
        "function f<T>(v: Option<int> | T) -> bool {\n    return match v {\n        case Some(x, y) => true\n        case _ => false\n    }\n}",
        "TPZ5004",
    );
    assert_code(
        "function f<T>(v: Option<int> | T) -> bool {\n    return match v {\n        case None(x) => true\n        case _ => false\n    }\n}",
        "TPZ5004",
    );
    // A concrete scrutinee with the wrong arity is rejected the same way.
    assert_code(
        "function f(v: Option<int>) -> int {\n    return match v {\n        case Some(x, y) => 1\n        case _ => 0\n    }\n}",
        "TPZ5004",
    );
}

#[test]
fn payload_coverage_exhausts_decidable_payloads() {
    // Some(true)/Some(false)/None exhausts Option<bool> without a
    // wildcard.
    assert_clean(
        "let x: Option<bool> = Some(true)\nmatch x {\n    case Some(true) => print(\"t\")\n    case Some(false) => print(\"f\")\n    case None => print(\"n\")\n}",
    );
    // A literal payload alone does not cover the open int domain.
    assert_code(
        "let x: Option<int> = Some(1)\nmatch x {\n    case Some(1) => print(\"one\")\n    case None => print(\"n\")\n}",
        "TPZ5021",
    );
}

#[test]
fn bare_none_patterns_are_constructor_patterns() {
    // The r2 counterexample: bare `None` parses as the §22.1
    // constructor pattern, not a catch-all binding, so it is
    // impossible on a decidable non-Option scrutinee.
    assert_code(
        "let n: int = 2\nmatch n {\n    case None => print(\"no\")\n    case _ => print(\"ok\")\n}",
        "TPZ5001",
    );
}

#[test]
fn bare_return_arms_check_unit_against_the_signature() {
    assert_code(
        "function pick(b: bool) -> int {\n    let n: int = match b {\n        case true => 1\n        case false => return\n    }\n    return n\n}",
        "TPZ5001",
    );
}

#[test]
fn top_level_match_return_arms_are_outside_function_scope() {
    assert_code_and_message(
        "match true {\n    case true => return 1\n    case _ => 0\n}",
        "TPZ5001",
        "`return` outside a function",
    );
    assert_code_and_message(
        "match true {\n    case true => return\n    case _ => 0\n}",
        "TPZ5001",
        "`return` outside a function",
    );
    assert_code_and_message(
        "match true {\n    case true => return 1\n    case _ => 0\n}\n0",
        "TPZ5001",
        "`return` outside a function",
    );
    assert_code_and_message(
        "if true {\n    match true {\n        case true => return 1\n        case _ => 0\n    }\n} else {\n    0\n}",
        "TPZ5001",
        "`return` outside a function",
    );
    assert_code_and_message(
        "let x = match true {\n    case true => return 1\n    case _ => 0\n}\nprint(\"{x}\")",
        "TPZ5001",
        "`return` outside a function",
    );
}

#[test]
fn function_local_match_return_arms_remain_in_function_scope() {
    assert_clean(
        "function pick(b: bool) -> int {\n    let n = match b {\n        case true => return 1\n        case false => 0\n    }\n    return n\n}\npick(false)",
    );
}

#[test]
fn bare_match_bindings_widen_literal_joins() {
    // With no context the join widens (§4), so the binding is int
    // and later assignment of another int checks.
    assert_clean(
        "let b: bool = true\nlet mut n = match b {\n    case true => 1\n    case false => 0\n}\nn = 2\nprint(\"{n}\")",
    );
}

// ---- match: pattern bindings against the scrutinee -----------------------

#[test]
fn constructor_patterns_bind_the_inner_type() {
    assert_clean(
        "let xs: Array<int> = [1]\nmatch xs.get(0) {\n    case Some(n) => print(\"{n + 1}\")\n    case None => print(\"empty\")\n}",
    );
    assert_code(
        "let xs: Array<int> = [1]\nmatch xs.get(0) {\n    case Some(n) => print(n)\n    case None => print(\"empty\")\n}",
        "TPZ5001",
    );
}

#[test]
fn result_patterns_bind_value_and_error() {
    assert_clean(
        "function f() -> Result<int, string> {\n    return Ok(1)\n}\nmatch f() {\n    case Ok(n) => print(\"{n + 1}\")\n    case Err(e) => print(e)\n}",
    );
    assert_code(
        "function f() -> Result<int, string> {\n    return Ok(1)\n}\nmatch f() {\n    case Ok(n) => print(\"{n}\")\n    case Err(e) => print(\"{e + 1}\")\n}",
        "TPZ5001",
    );
}

#[test]
fn record_patterns_bind_field_types() {
    assert_clean(
        "let user: { name: string, age: int } = { name: \"t\", age: 3 }\nmatch user {\n    case { name, age } => print(\"{name} {age + 1}\")\n}",
    );
    assert_code(
        "let user: { name: string, age: int } = { name: \"t\", age: 3 }\nmatch user {\n    case { name } => print(\"{name + 1}\")\n}",
        "TPZ5001",
    );
    assert_code(
        "let user: { name: string } = { name: \"t\" }\nmatch user {\n    case { email } => print(\"{email}\")\n}",
        "TPZ5006",
    );
}

#[test]
fn list_patterns_bind_elements_and_rest() {
    assert_clean(
        "let xs: Array<int> = [1, 2, 3]\nmatch xs {\n    case [first, ..rest] => print(\"{first + 1} {rest.length}\")\n    case [] => print(\"empty\")\n    case _ => print(\"other\")\n}",
    );
    assert_code(
        "let xs: Array<int> = [1, 2]\nmatch xs {\n    case [first] => print(first)\n    case _ => print(\"other\")\n}",
        "TPZ5001",
    );
}

#[test]
fn guards_must_be_boolean() {
    assert_code(
        "let n: int = 1\nmatch n {\n    case x if x => print(\"{x}\")\n    case _ => print(\"other\")\n}",
        "TPZ5001",
    );
}

#[test]
fn impossible_literal_patterns_are_tpz5001() {
    assert_code(
        "let n: int = 1\nmatch n {\n    case \"one\" => print(\"one\")\n    case _ => print(\"other\")\n}",
        "TPZ5001",
    );
}

#[test]
fn impossible_constructor_patterns_are_tpz5001() {
    assert_code(
        "function f() -> Result<int, string> {\n    return Ok(1)\n}\nmatch f() {\n    case Some(n) => print(\"{n}\")\n    case _ => print(\"other\")\n}",
        "TPZ5001",
    );
}

// ---- type patterns -------------------------------------------------------

#[test]
fn type_patterns_narrow_union_scrutinees() {
    assert_clean(
        "let v: int | string = 1\nmatch v {\n    case n: int => print(\"{n + 1}\")\n    case s: string => print(s)\n}",
    );
}

#[test]
fn disjoint_type_patterns_are_tpz5001() {
    assert_code(
        "let v: int | string = 1\nmatch v {\n    case b: bool => print(\"{b}\")\n    case _ => print(\"other\")\n}",
        "TPZ5001",
    );
}

// ---- exhaustiveness (TPZ5021) ---------------------------------------------

#[test]
fn bool_matches_need_both_arms() {
    assert_code(
        "let b: bool = true\nlet s = match b {\n    case true => \"yes\"\n}\nprint(s)",
        "TPZ5021",
    );
    assert_clean(
        "let b: bool = true\nlet s = match b {\n    case true => \"yes\"\n    case false => \"no\"\n}\nprint(s)",
    );
}

#[test]
fn option_matches_need_some_and_none() {
    assert_code(
        "let xs: Array<int> = [1]\nmatch xs.get(0) {\n    case Some(n) => print(\"{n}\")\n}",
        "TPZ5021",
    );
    assert_clean(
        "let xs: Array<int> = [1]\nmatch xs.get(0) {\n    case Some(n) => print(\"{n}\")\n    case None => print(\"empty\")\n}",
    );
}

#[test]
fn result_matches_need_ok_and_err() {
    assert_code(
        "function f() -> Result<int, string> {\n    return Ok(1)\n}\nmatch f() {\n    case Ok(n) => print(\"{n}\")\n}",
        "TPZ5021",
    );
}

#[test]
fn literal_union_matches_cover_every_member() {
    assert_code(
        "type Mode = \"on\" | \"off\"\nlet m: Mode = \"on\"\nmatch m {\n    case \"on\" => print(\"on\")\n}",
        "TPZ5021",
    );
    assert_clean(
        "type Mode = \"on\" | \"off\"\nlet m: Mode = \"on\"\nmatch m {\n    case \"on\" => print(\"on\")\n    case \"off\" => print(\"off\")\n}",
    );
}

// ---- §3 (v5.3) user enums — payload-less MVP --------------------------------

#[test]
fn enum_match_needs_every_variant() {
    // The negative fixture: a match missing `Blue` is non-exhaustive.
    assert_code(
        "enum Color { Red, Blue }\nlet c: Color = Color.Red\nmatch c {\n    case Red => print(\"r\")\n}",
        "TPZ5021",
    );
    // Covering both variants is exhaustive.
    assert_clean(
        "enum Color { Red, Blue }\nlet c: Color = Color.Red\nmatch c {\n    case Red => print(\"r\")\n    case Blue => print(\"b\")\n}",
    );
}

#[test]
fn enum_wildcard_discharges_exhaustiveness() {
    // `_` is the catch-all an enum scrutinee admits — it discharges exhaustiveness.
    assert_clean(
        "enum Color { Red, Blue }\nlet c: Color = Color.Red\nmatch c {\n    case Red => print(\"r\")\n    case _ => print(\"other\")\n}",
    );
}

#[test]
fn enum_bare_name_is_position_sensitive() {
    // v5.4: a bare name over an enum is POSITION-SENSITIVE. A name that IS a
    // declared variant is a refutable variant pattern at any position. A NON-variant
    // bare name at a NESTED payload subpattern position BINDS (essential to
    // destructure an enum-typed payload, `Bin(op, l, r)` binds `op: Op`)...
    assert_clean(
        "enum Op { Add, Mul }\nenum Expr { Num(int), Bin(Op, Expr, Expr) }\nfunction f(e: Expr) -> int {\n    match e {\n        case Num(n) => n\n        case Bin(op, l, r) => 0\n    }\n}\nprint(\"{f(Expr.Num(1))}\")",
    );
    // ...but at the TOP LEVEL of a match arm it is a likely TYPO and is rejected
    // (the intentional catch-all is `_`).
    assert_code(
        "enum Color { Red, Blue }\nlet c: Color = Color.Red\nmatch c {\n    case Red => 1\n    case Bloo => 2\n}",
        "TPZ5001",
    );
    assert_code(
        "enum Color { Red, Blue }\nlet c: Color = Color.Red\nmatch c {\n    case other => 0\n}",
        "TPZ5001",
    );
    // A `let`/`for` destructuring (a BINDING context, not a match arm) binds a
    // bare name over an enum without the typo gate.
    assert_clean(
        "enum Color { Red, Blue }\nlet cs: Array<Color> = [Color.Red]\nfor c in cs {\n    print(\"{c}\")\n}",
    );
}

#[test]
fn enum_construction_types_nominally() {
    assert_clean("enum Color { Red, Blue }\nlet c: Color = Color.Red\nprint(\"{c}\")");
    // A nominal mismatch: an enum is not assignable to a same-shaped enum.
    assert_code(
        "enum A { X }\nenum B { X }\nlet a: A = A.X\nlet b: B = a",
        "TPZ5001",
    );
    // An enum is not an int (nor vice versa).
    assert_code(
        "enum Color { Red, Blue }\nlet n: int = Color.Red",
        "TPZ5001",
    );
}

#[test]
fn enum_unknown_variant_is_rejected() {
    assert_code(
        "enum Color { Red, Blue }\nlet c: Color = Color.Green",
        "TPZ5006",
    );
}

#[test]
fn enum_single_payload_variants_work() {
    // A SINGLE-payload variant (mixed with payload-less) declares, constructs,
    // and matches with payload binding.
    assert_clean(
        "enum Shape { Circle(int), Square(int), Dot }\nlet s: Shape = Shape.Circle(3)\nlet n = match s {\n    case Circle(r) => r\n    case Square(side) => side\n    case Dot => 0\n}\nprint(\"{n}\")",
    );
    // A string payload.
    assert_clean(
        "enum Msg { Text(string), Empty }\nlet m: Msg = Msg.Text(\"hi\")\nmatch m {\n    case Text(s) => print(s)\n    case Empty => print(\"none\")\n}",
    );
}

#[test]
fn enum_payload_type_and_arity_are_checked() {
    // Payload TYPE mismatch at construction.
    assert_code(
        "enum Shape { Circle(int), Dot }\nlet s: Shape = Shape.Circle(\"hi\")",
        "TPZ5001",
    );
    // Arity: a payloadful variant constructed with no payload.
    assert_code(
        "enum Shape { Circle(int), Dot }\nlet s: Shape = Shape.Circle",
        "TPZ5004",
    );
    // Arity: a payload-less variant constructed with a payload.
    assert_code(
        "enum Shape { Circle(int), Dot }\nlet s: Shape = Shape.Dot(5)",
        "TPZ5004",
    );
    // Pattern arity: a bare name over a payloadful variant.
    assert_code(
        "enum Shape { Circle(int), Dot }\nlet s: Shape = Shape.Dot\nmatch s {\n    case Circle => 1\n    case Dot => 0\n}",
        "TPZ5004",
    );
    // Pattern arity: a subpattern over a payload-less variant.
    assert_code(
        "enum Shape { Circle(int), Dot }\nlet s: Shape = Shape.Dot\nmatch s {\n    case Circle(r) => r\n    case Dot(x) => x\n}",
        "TPZ5004",
    );
}

#[test]
fn enum_payload_exhaustiveness_is_payload_aware() {
    // `Circle(1)` does NOT exhaust `Circle(int)` — still non-exhaustive.
    assert_code(
        "enum Shape { Circle(int), Dot }\nlet s: Shape = Shape.Dot\nmatch s {\n    case Circle(1) => print(\"one\")\n    case Dot => print(\"dot\")\n}",
        "TPZ5021",
    );
    // `Circle(_)` covers `Circle`; with `Dot` the match is exhaustive.
    assert_clean(
        "enum Shape { Circle(int), Dot }\nlet s: Shape = Shape.Dot\nmatch s {\n    case Circle(_) => print(\"c\")\n    case Dot => print(\"dot\")\n}",
    );
}

#[test]
fn enum_multi_payload_is_v54_gated() {
    // A MULTI-payload tuple variant (2+ types) is a v5.4 feature: rejected below
    // v5.4, accepted at v5.4.
    assert!(
        check_at(
            "enum Shape { Circle(int, string), Dot }\n0",
            LangVersion::V5_3
        )
        .iter()
        .any(|d| d.starts_with("TPZ5022")),
        "multi-payload must be rejected at v5.3"
    );
    assert!(
        check_at(
            "enum Shape { Circle(int, string), Dot }\n0",
            LangVersion::V5_4
        )
        .is_empty(),
        "multi-payload must be accepted at v5.4"
    );
    // A payload on a payload-LESS variant's construction is an arity error.
    assert_code(
        "enum Color { Red, Blue }\nlet c = Color.Red(1)\nprint(\"{c}\")",
        "TPZ5004",
    );
}

#[test]
fn enum_multi_payload_construct_match_arity() {
    // Construct + N-subpattern match of a multi-payload variant at v5.4.
    assert_clean(
        "enum Pair { P(int, int) }\nlet p: Pair = Pair.P(1, 2)\nlet s = match p {\n    case P(a, b) => a + b\n}\nprint(\"{s}\")",
    );
    // Wrong construction arity.
    assert_code(
        "enum Pair { P(int, int) }\nlet p: Pair = Pair.P(1, 2, 3)\nprint(\"{p}\")",
        "TPZ5004",
    );
    // Wrong pattern arity.
    assert_code(
        "enum Pair { P(int, int) }\nlet p: Pair = Pair.P(1, 2)\nmatch p {\n    case P(a, b, c) => print(\"{a}\")\n}",
        "TPZ5004",
    );
}

#[test]
fn enum_recursive_and_mutual_form_and_check() {
    // A self-recursive enum (the self-host AST, arena-free) checks clean.
    assert_clean(
        "enum Op { Add, Mul }\nenum Expr { Num(int), Bin(Op, Expr, Expr) }\nfunction eval(e: Expr) -> int {\n    match e {\n        case Num(n) => n\n        case Bin(op, l, r) => match op {\n            case Add => eval(l) + eval(r)\n            case Mul => eval(l) * eval(r)\n        }\n    }\n}\nprint(\"{eval(Expr.Bin(Op.Add, Expr.Num(1), Expr.Num(2)))}\")",
    );
    // A mutually-recursive pair checks clean (two-phase formation).
    assert_clean(
        "enum A { Stop, GoA(B) }\nenum B { GoB(A) }\nfunction d(a: A) -> int {\n    match a {\n        case Stop => 0\n        case GoA(b) => match b {\n            case GoB(inner) => 1 + d(inner)\n        }\n    }\n}\nprint(\"{d(A.Stop)}\")",
    );
}

#[test]
fn nested_enum_declaration_is_rejected_explicitly() {
    assert_code(
        "function build() -> int {\n    enum Local { Value }\n    1\n}\nprint(\"{build()}\")",
        "TPZ5022",
    );
}

#[test]
fn enum_multi_payload_exhaustiveness_is_conservative() {
    // `Bin(_, _, _)` (all irrefutable) exhausts `Bin`.
    assert_clean(
        "enum Op { Add }\nenum Expr { Num(int), Bin(Op, Expr, Expr) }\nfunction f(e: Expr) -> int {\n    match e {\n        case Num(n) => n\n        case Bin(op, l, r) => 0\n    }\n}\nprint(\"{f(Expr.Num(1))}\")",
    );
    // A refutable position (`Num(x)` at the 2nd Expr slot) does NOT exhaust `Bin`.
    assert_code(
        "enum Op { Add }\nenum Expr { Num(int), Bin(Op, Expr, Expr) }\nfunction f(e: Expr) -> int {\n    match e {\n        case Num(n) => n\n        case Bin(op, Num(x), r) => x\n    }\n}\nprint(\"{f(Expr.Num(1))}\")",
        "TPZ5021",
    );
}
