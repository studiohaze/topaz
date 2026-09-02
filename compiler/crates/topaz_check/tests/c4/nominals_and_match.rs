use super::*;

// ---- §3 (v5.4) nominal records ----------------------------------------------

#[test]
fn record_construct_and_field_and_pattern() {
    // Construct, annotate, field-access, and destructure a nominal record.
    assert_clean(
        "record User { name: string, age: int }\nlet u: User = User { name: \"a\", age: 1 }\nlet s = match u {\n    case User { name, age } => \"{name}{age}\"\n}\nprint(\"{s}{u.name}\")",
    );
}

#[test]
fn nominal_record_pattern_narrows_union_members_by_declaration() {
    assert_clean(
        "record User { name: string, age: int }\nrecord Admin { level: int }\nfunction label(value: User | Admin | null) -> string {\n    match value {\n        case User { name, age: 36 } => name\n        case User { name } => \"other-{name}\"\n        case Admin { level } => \"admin-{level}\"\n        case null => \"none\"\n    }\n}\nlet user: User = User { name: \"Ada\", age: 36 }\nprint(label(user))",
    );
}

#[test]
fn nominal_record_pattern_exhausts_same_base_generic_union() {
    assert_clean(
        "record Box<T> { value: T }\nfunction show(value: Box<int> | Box<string>) -> string {\n    match value {\n        case Box { value } => match value {\n            case n: int => \"{n}\"\n            case s: string => s\n        }\n    }\n}\nlet box: Box<int> = Box { value: 7 }\nprint(show(box))",
    );
}

#[test]
fn nominal_record_union_exhaustiveness_keeps_nonmatching_members() {
    assert_code_and_message(
        "record User { name: string }\nfunction label(value: User | null) -> string {\n    match value {\n        case User { name } => name\n    }\n}",
        "TPZ5021",
        "`null`",
    );
    assert_clean(
        "record User { name: string }\nfunction label(value: User | null) -> string {\n    match value {\n        case User { name } => name\n        case null => \"none\"\n    }\n}\nprint(label(null))",
    );
}

#[test]
fn nominal_record_pattern_head_must_resolve_before_backend_selection() {
    assert_code_and_message(
        "record User { name: string }\nlet user: User = User { name: \"Ada\" }\nlet label = match user {\n    case Usre { name } => name\n    case _ => \"none\"\n}\nprint(label)",
        "TPZ5002",
        "unbound nominal record pattern head `Usre`; did you mean `User`?",
    );
    assert_code_and_message(
        "let classify = value => match value {\n    case Missing { name } => name\n    case _ => \"none\"\n}\nprint(classify({ name: \"Ada\" }))",
        "TPZ5002",
        "unbound nominal record pattern head `Missing`",
    );
}

#[test]
fn nominal_record_pattern_rejects_rigid_scrutinee_identity() {
    assert_code_and_message(
        "record User { name: string }\nfunction label<T>(value: T) -> string {\n    match value {\n        case User { name } => name\n        case _ => \"none\"\n    }\n}",
        "TPZ5001",
        "cannot establish nominal identity for rigid scrutinee type",
    );
}

#[test]
fn nominal_record_pattern_rejects_duplicate_source_fields() {
    assert_code_and_message(
        "record User { name: string }\nlet user: User = User { name: \"Ada\" }\nlet label = match user {\n    case User { name: first, name: second } => \"{first}:{second}\"\n}\nprint(label)",
        "TPZ5008",
        "nominal record pattern field `name` is specified more than once",
    );
}

#[test]
fn record_construction_checks_fields() {
    // A wrong field TYPE is a mismatch.
    assert_code(
        "record User { name: string, age: int }\nlet u: User = User { name: \"a\", age: \"x\" }",
        "TPZ5001",
    );
    // An UNKNOWN field is rejected.
    assert_code(
        "record User { name: string, age: int }\nlet u: User = User { name: \"a\", age: 1, oops: 2 }",
        "TPZ5006",
    );
    // A MISSING non-default field is required.
    assert_code(
        "record User { name: string, age: int }\nlet u: User = User { name: \"a\" }",
        "TPZ5004",
    );
}

#[test]
fn record_defaults_allow_omission() {
    // A field WITH a default may be omitted.
    assert_clean(
        "record Config { host: string, port: int = 8080 }\nlet c: Config = Config { host: \"h\" }\nprint(\"{c.port}\")",
    );
    // A default must conform to the field's declared type.
    assert_code("record Bad { n: int = \"oops\" }\n0", "TPZ5001");
}

#[test]
fn record_defaults_have_no_self_or_sibling_scope() {
    // The second `first` resolves to the ordinary defining-module binding, not to
    // the earlier record field. Closed-unit unbound rejection is pinned separately.
    assert_clean(
        "let first = 9\nrecord Values { first: int = 1, second: int = first }\nlet v: Values = Values {}\nprint(\"{v.first}:{v.second}\")",
    );
}

#[test]
fn generic_record_defaults_use_expected_type_context() {
    assert_clean(
        "record Box<T> { value: T | null = null }\n\
         record Outer { box: Box<int> }\n\
         function take(box: Box<int>) -> int { 1 }\n\
         function make() -> Box<int> { Box {} }\n\
         function choose(flag: bool) -> Box<int> {\n    match flag {\n        case true => Box {}\n        case false => Box {}\n    }\n}\n\
         let direct: Box<int> = Box {}\n\
         let nested: Outer = Outer { box: Box {} }\n\
         let array: Array<Box<int>> = [Box {}]\n\
         let matched: Box<int> = choose(true)\n\
         let called = take(Box {})\n\
         let returned: Box<int> = make()\n\
         print(\"{direct}/{nested}/{array}/{matched}/{called}/{returned}\")",
    );
    assert_code(
        "record Box<T> { value: T | null = null }\nlet ambiguous = Box {}",
        "TPZ5022",
    );
}

#[test]
fn nominal_record_spread_preserves_exact_expected_instance() {
    assert_clean(
        "record Box<T> { value: T }\n\
         let base: Box<int> = Box { value: 1 }\n\
         let next: Box<int> = Box { ...base, }\n\
         print(\"{next.value}\")",
    );
    assert_code(
        "record Box<T> { value: T }\n\
         let base: Box<int> = Box { value: 1 }\n\
         let next = Box { ...base }",
        "TPZ5022",
    );
    assert_code(
        "record Box<T> { value: T }\n\
         let base: Box<int> = Box { value: 1 }\n\
         let next: Box<string> = Box { ...base }",
        "TPZ5001",
    );
}

#[test]
fn nominal_record_spread_rejects_non_nominal_and_bad_fields() {
    assert_code(
        "record User { name: string, age: int }\n\
         record Other { name: string, age: int }\n\
         let other: Other = Other { name: \"Ada\", age: 36 }\n\
         let user: User = User { ...other }",
        "TPZ5001",
    );
    assert_code(
        "record User { name: string, age: int }\n\
         let base: User = User { name: \"Ada\", age: 36 }\n\
         let User = { name: \"shadow\", age: 0 }\n\
         let next = User { ...base, age: 37 }",
        "TPZ5001",
    );
    assert_code(
        "record User { name: string, age: int }\n\
         let base: User = User { name: \"Ada\", age: 36 }\n\
         let next = base { age: 37 }",
        "TPZ5001",
    );
    assert_code(
        "record User { name: string, age: int }\n\
         let base: User = User { name: \"Ada\", age: 36 }\n\
         let next: User = User { ...base, age: 37, age: 38 }",
        "TPZ5008",
    );
    assert_code(
        "record User { name: string, age: int }\n\
         let base: User = User { name: \"Ada\", age: 36 }\n\
         let next: User = User { ...base, missing: 1 }",
        "TPZ5006",
    );
}

#[test]
fn record_is_nominal_not_structural() {
    // A structural literal is NOT assignable to a nominal record param/binding.
    assert_code(
        "record User { name: string, age: int }\nlet u: User = { name: \"a\", age: 1 }",
        "TPZ5001",
    );
    // Two same-shaped records are DISTINCT nominal types.
    assert_code(
        "record A { x: int }\nrecord B { x: int }\nlet a: A = A { x: 1 }\nlet b: B = a",
        "TPZ5001",
    );
}

#[test]
fn record_name_collisions_are_rejected() {
    // A record name colliding with an enum / another record / a builtin is rejected.
    assert_code("enum E { X }\nrecord E { a: int }\n0", "TPZ5008");
    assert_code("record Array { a: int }\n0", "TPZ5022");
    assert_code("record R { a: int }\nrecord R { b: int }\n0", "TPZ5008");
}

#[test]
fn stdlib_receiver_names_are_reserved_for_user_methods() {
    for name in ["find", "parent", "sortBy", "update", "set", "named"] {
        assert_code(
            &format!(
                "record P {{ x: int }}\nimpl P {{ function {name}(self) -> int {{ self.x }} }}\n0"
            ),
            "TPZ5022",
        );
    }
}

#[test]
fn record_exhaustiveness_is_irrefutable_only() {
    // An all-binding record pattern exhausts the record's single shape.
    assert_clean(
        "record P { x: int, y: int }\nlet p: P = P { x: 1, y: 2 }\nlet s = match p {\n    case P { x, y } => x + y\n}\nprint(\"{s}\")",
    );
    // A refutable field subpattern (a literal) does NOT exhaust the record.
    assert_code(
        "record P { x: int, y: int }\nlet p: P = P { x: 1, y: 2 }\nlet s = match p {\n    case P { x: 0, y } => y\n}\nprint(\"{s}\")",
        "TPZ5021",
    );
}

#[test]
fn record_and_enum_form_mutually() {
    // A record containing an enum, and an enum carrying a record — both formed by
    // the UNIFIED nominal collection pass (record↔enum mutual reference).
    assert_clean(
        "enum Status { Active }\nrecord Account { id: int, status: Status }\nlet a: Account = Account { id: 1, status: Status.Active }\nprint(\"{a}\")",
    );
    assert_clean(
        "record Point { x: int, y: int }\nenum Shape { At(Point) }\nlet s: Shape = Shape.At(Point { x: 1, y: 2 })\nprint(\"{s}\")",
    );
}

#[test]
fn concrete_generic_nominals_construct_and_pattern() {
    assert_clean(
        "record Box<T> { value: T }\nlet b: Box<int> = Box { value: 1 }\nlet n: int = b.value\nlet sbox: Box<string> = Box { value: \"ok\" }\nlet s: string = match sbox {\n    case Box { value } => value\n}\nprint(\"{n}{s}\")",
    );
    assert_clean(
        "enum Maybe<T> { Missing, Present(T) }\nlet m: Maybe<int> = Maybe.Present(5)\nlet n: int = match m {\n    case Present(x) => x\n    case Missing => 0\n}\nlet none: Maybe<string> = Maybe.Missing\nprint(\"{n}{none}\")",
    );
    assert_clean(
        "newtype Id<T> = T\nlet id: Id<string> = Id(\"a\")\nlet s: string = id.value()\nlet out = match id {\n    case Id(v) => v\n}\nprint(\"{s}{out}\")",
    );
}

#[test]
fn generic_nominal_construction_needs_context() {
    assert_code(
        "record Box<T> { value: T }\nlet b = Box { value: 1 }",
        "TPZ5022",
    );
    assert_code(
        "enum Maybe<T> { Missing, Present(T) }\nlet m = Maybe.Present(1)",
        "TPZ5022",
    );
    assert_code("newtype Id<T> = T\nlet id = Id(1)", "TPZ5022");
}

#[test]
fn generic_nominal_pass_through_substitutes_record_args() {
    assert_clean(
        "record Html<Msg> { message: Msg }\nfunction wrap<T>(value: T) -> Html<T> {\n    Html { message: value }\n}\nlet h: Html<int> = wrap(7)\nlet n: int = h.message\nprint(\"{n}\")",
    );
}

#[test]
fn generic_nominal_pass_through_substitutes_enum_args() {
    assert_clean(
        "enum Maybe<T> { Missing, Present(T) }\nfunction just<T>(value: T) -> Maybe<T> {\n    Maybe.Present(value)\n}\nlet m: Maybe<int> = just(5)\nlet n: int = match m {\n    case Present(x) => x\n    case Missing => 0\n}\nprint(\"{n}\")",
    );
    assert_clean(
        "enum Maybe<T> { Missing, Present(T) }\nfunction none<T>() -> Maybe<T> {\n    Maybe.Missing\n}\nlet m: Maybe<int> = none()\nlet n: int = match m {\n    case Present(x) => x\n    case Missing => 0\n}\nprint(\"{n}\")",
    );
    assert_clean(
        "enum Command<Msg> { SetText(string, string), Dispatch(Msg) }\nfunction setText<M>(selector: string, text: string) -> Command<M> {\n    Command.SetText(selector, text)\n}\nfunction dispatch<M>(message: M) -> Command<M> {\n    Command.Dispatch(message)\n}\nlet a: Command<int> = setText(\"#status\", \"ok\")\nlet b: Command<int> = dispatch(7)\nprint(\"{a}/{b}\")",
    );
}

#[test]
fn generic_nominal_pass_through_substitutes_newtype_args() {
    assert_clean(
        "newtype Id<T> = T\nfunction wrap<T>(value: T) -> Id<T> {\n    Id(value)\n}\nlet id: Id<string> = wrap(\"a\")\nlet s: string = id.value()\nprint(s)",
    );
}

#[test]
fn concrete_generic_nominals_are_invariant() {
    assert_code(
        "record Box<T> { value: T }\nlet b: Box<int> = Box { value: \"bad\" }",
        "TPZ5001",
    );
    assert_code(
        "record Box<T> { value: T }\nlet x: Box<int> = Box { value: 1 }\nlet y: Box<string> = x",
        "TPZ5001",
    );
    assert_code(
        "enum Maybe<T> { Present(T) }\nlet m: Maybe<int> = Maybe.Present(\"bad\")",
        "TPZ5001",
    );
    assert_code(
        "newtype Id<T> = T\nlet id: Id<int> = Id(\"bad\")",
        "TPZ5001",
    );
}

#[test]
fn record_field_access_is_typed() {
    // A nominal record's field access types as its DECLARED field type, so a
    // wrong use (`u.name` where `name: string` used as an `int`) is a CHECK error.
    assert_code(
        "record User { name: string, age: int }\nfunction f(u: User) -> int { return u.name }",
        "TPZ5001",
    );
    // The right field type is clean.
    assert_clean(
        "record User { name: string, age: int }\nfunction f(u: User) -> int { return u.age }",
    );
    // An unknown field access is a NO_FIELD error.
    assert_code(
        "record User { name: string }\nfunction f(u: User) -> string { return u.oops }",
        "TPZ5006",
    );
    // A callable (Func-typed) field can be CALLED.
    assert_clean("record Box { f: () -> int }\nfunction call(b: Box) -> int { return b.f() }");
}

#[test]
fn record_comparability_consults_field_types() {
    // A nominal record with a NON-comparable field (`Map`) is NOT comparable — a
    // CHECK error (the runtime eq would otherwise fault).
    assert_code(
        "record Box { m: Map<string, int> }\nlet left: Box = Box { m: Map.new() }\nlet right: Box = Box { m: Map.new() }\nlet eq = left == right",
        "TPZ5007",
    );
    // A record with only comparable fields IS comparable.
    assert_clean(
        "record P { x: int, y: string }\nlet eq = P { x: 1, y: \"a\" } == P { x: 1, y: \"b\" }\nprint(\"{eq}\")",
    );
}

#[test]
fn record_keyability_is_static_and_transitive() {
    assert_code(
        "record Bad { values: Map<string, int> }\nlet m: Map<Bad, int> = Map.new()",
        "TPZ5007",
    );
    assert_code(
        "record Bad { values: Map<string, int> }\nrecord Outer { inner: Bad }\nlet s: Set<Outer> = Set.of()",
        "TPZ5007",
    );
    assert_clean(
        "record Inner { value: int }\nrecord Outer { inner: Inner, label: string }\nlet m: Map<Outer, int> = Map.new()\nlet s: Set<Outer> = Set.of()\nprint(\"{m.keys.length}/{s}\")",
    );
}

#[test]
fn record_empty_construction() {
    // A zero-field record constructs with `R {}`.
    assert_clean("record Empty { }\nlet e: Empty = Empty {}\nprint(\"{e}\")");
    // An all-default record constructs with `R {}` (all fields defaulted).
    assert_clean(
        "record Config { host: string = \"h\", port: int = 80 }\nlet c: Config = Config {}\nprint(\"{c}\")",
    );
}

#[test]
fn record_is_v54_only() {
    // `record` is a v5.4-only feature: at v5.3 `record User { … }` does NOT parse
    // as a record declaration (`record` is an ordinary identifier there), so the
    // source is rejected at the parse stage.
    let v53 = parse_with_options(
        FileId(0),
        "record User { name: string }\n0",
        ParseOptions {
            language_version: LangVersion::V5_3,
        },
    );
    assert!(
        !v53.diagnostics.is_empty(),
        "a record decl must NOT parse at v5.3"
    );
    // At v5.4 the same source is a clean record declaration (parses + checks).
    assert!(
        check_at("record User { name: string }\n0", LangVersion::V5_4).is_empty(),
        "record must be accepted at v5.4"
    );
}

#[test]
fn record_declarations_are_module_top_level_only() {
    for version in [LangVersion::V5_4, LangVersion::V5_5] {
        let diags = check_at(
            "function f() -> int {\n    record Hidden { value: int }\n    0\n}\nf()",
            version,
        );
        assert!(
            diags.iter().any(|diag| {
                diag.starts_with("TPZ5022")
                    && diag.contains("record declarations are module-top-level only")
            }),
            "expected explicit nested-record rejection at {version:?}, got: {diags:?}"
        );
    }
}

#[test]
fn newtype_declarations_are_module_top_level_only() {
    for version in [LangVersion::V5_4, LangVersion::V5_5] {
        let diags = check_at(
            "function f() -> int {\n    newtype Hidden = int\n    0\n}\nf()",
            version,
        );
        assert!(
            diags.iter().any(|diag| {
                diag.starts_with("TPZ5022")
                    && diag.contains("newtype declarations are module-top-level only")
            }),
            "expected explicit nested-newtype rejection at {version:?}, got: {diags:?}"
        );
    }
}

#[test]
fn newtype_remains_contextual_outside_a_declaration_head() {
    assert_clean("let newtype = 5\nlet n: int = newtype\nprint(\"{n}\")");
}

#[test]
fn enum_in_union_payload_is_a_later_slice() {
    // A single-payload variant whose payload is a UNION CONTAINING an enum
    // (`Wrap(Color | int)`) is rejected: a bare subpattern there is ambiguous
    // (variant match vs binding) and would diverge run≢build.
    assert_code(
        "enum Color { Red, Blue }\nenum Box { Wrap(Color | int) }\n0",
        "TPZ5022",
    );
    // A union payload with NO enum (`int | string`) is fine — a bare subpattern
    // there is an unambiguous binding.
    assert_clean(
        "enum Box { Wrap(int | string) }\nlet b: Box = Box.Wrap(7)\nmatch b {\n    case Wrap(x) => print(\"{x}\")\n}",
    );
}

#[test]
fn enum_nested_payload_exhaustiveness_counts_all_variants() {
    // A nested enum payload is exhaustive when every variant is covered — it must
    // NOT over-report the outer variant as missing.
    assert_clean(
        "enum Color { Red, Blue }\nenum Box { Wrap(Color) }\nlet b: Box = Box.Wrap(Color.Red)\nmatch b {\n    case Wrap(Red) => print(\"r\")\n    case Wrap(Blue) => print(\"b\")\n}",
    );
    // Missing a nested variant is still non-exhaustive.
    assert_code(
        "enum Color { Red, Blue }\nenum Box { Wrap(Color) }\nlet b: Box = Box.Wrap(Color.Red)\nmatch b {\n    case Wrap(Red) => print(\"r\")\n}",
        "TPZ5021",
    );
}

#[test]
fn enum_bare_reference_to_payloadful_variant_is_an_arity_error() {
    // A bare reference (NO call) to a payloadful variant is an arity error in
    // checked mode (and is handled consistently under `--unchecked`).
    assert_code(
        "enum Shape { Circle(int), Dot }\nlet s = Shape.Circle\nprint(\"{s}\")",
        "TPZ5004",
    );
}

#[test]
fn enum_variant_cannot_use_a_reserved_prelude_name() {
    // A variant named `None`/`Some`/`Ok`/`Err` would diverge run≢build (both
    // engines treat those as the prelude constructor), so the decl is rejected.
    assert_code("enum Maybe { None, Some }\n0", "TPZ5022");
    assert_code("enum E { Ok, Err }\n0", "TPZ5022");
    // (Literals `true`/`false`/`null` are not Idents, so the PARSER already
    // rejects them as variant names — no check-level gate needed.)
}

#[test]
fn enum_duplicate_variant_is_rejected() {
    assert_code("enum E { X, X }\n0", "TPZ5008");
}

#[test]
fn enum_name_collisions_are_rejected() {
    // Colliding with a builtin generic ctor / primitive / opaque library type.
    assert_code("enum Array { X }\n0", "TPZ5022");
    assert_code("enum Option { X }\n0", "TPZ5022");
    assert_code("enum JSONValue { X }\n0", "TPZ5022");
    // Colliding with a declared type alias.
    assert_code("type E = int\nenum E { X }\n0", "TPZ5022");
    // A duplicate enum declaration.
    assert_code("enum E { X }\nenum E { Y }\n0", "TPZ5008");
}

#[test]
fn wildcards_and_bindings_discharge_exhaustiveness() {
    assert_clean(
        "let b: bool = true\nmatch b {\n    case true => print(\"yes\")\n    case _ => print(\"no\")\n}",
    );
    assert_clean(
        "let xs: Array<int> = [1]\nmatch xs.get(0) {\n    case Some(n) => print(\"{n}\")\n    case other => print(\"{other}\")\n}",
    );
}

#[test]
fn guarded_arms_do_not_count_as_coverage() {
    assert_code(
        "let b: bool = true\nmatch b {\n    case true => print(\"yes\")\n    case false if b => print(\"no\")\n}",
        "TPZ5021",
    );
}

#[test]
fn or_patterns_accumulate_coverage() {
    assert_clean(
        "type Mode = \"on\" | \"off\" | \"auto\"\nlet m: Mode = \"on\"\nmatch m {\n    case \"on\" | \"off\" => print(\"manual\")\n    case \"auto\" => print(\"auto\")\n}",
    );
}

#[test]
fn covering_type_patterns_are_irrefutable() {
    assert_clean("let n: int = 1\nmatch n {\n    case x: int => print(\"{x}\")\n}");
}

#[test]
fn undecidable_domains_stay_silent() {
    // int and string have open domains: no 5021.
    assert_clean("let n: int = 1\nmatch n {\n    case 1 => print(\"one\")\n}");
    assert_clean("let s: string = \"a\"\nmatch s {\n    case \"a\" => print(\"a\")\n}");
}

// ---- match typing: result and context ------------------------------------

#[test]
fn match_results_join_the_arm_types() {
    assert_clean(
        "let b: bool = true\nlet n: int = match b {\n    case true => 1\n    case false => 0\n}\nprint(\"{n}\")",
    );
    assert_code(
        "let b: bool = true\nlet n: int = match b {\n    case true => 1\n    case false => \"zero\"\n}\nprint(\"{n}\")",
        "TPZ5001",
    );
}

#[test]
fn match_arms_are_context_sites() {
    // §22.1: the binding annotation reaches into each arm, so the
    // unsolved constructors resolve without per-arm annotations.
    assert_clean(
        "let b: bool = true\nlet xs: Array<int> = match b {\n    case true => []\n    case false => Array.of(1)\n}\nprint(\"{xs}\")",
    );
}

#[test]
fn match_joins_preserve_literal_arm_types() {
    // `return match …` against a literal-union return type: the
    // join must not widen the arms to string.
    assert_clean(
        "type Light = \"red\" | \"yellow\" | \"green\"\nfunction next(light: Light) -> Light {\n    return match light {\n        case \"red\" => \"green\"\n        case \"green\" => \"yellow\"\n        case _ => \"red\"\n    }\n}",
    );
}

#[test]
fn block_arms_ending_in_return_diverge() {
    // The §13 canonical pattern: a None arm that returns out of the
    // enclosing function contributes nothing to the join.
    assert_clean(
        "function parse(text: string) -> Result<int, string> {\n    let number = match toInt(text) {\n        case Some(value) => value\n        case None => { return Err(\"bad: {text}\") }\n    }\n    return Ok(number + 1)\n}",
    );
}

#[test]
fn foreign_scrutinees_stay_silent() {
    // An undeclared named type forms as Foreign: patterns and
    // exhaustiveness on it cannot be judged.
    assert_clean(
        "function label(status: OrderStatus) -> string {\n    match status {\n        case \"created\" => \"Awaiting payment\"\n        case \"shipped\" => \"On the way\"\n    }\n}",
    );
}

#[test]
fn return_arms_diverge_and_check_against_the_signature() {
    assert_clean(
        "function pick(b: bool) -> int {\n    let n: int = match b {\n        case true => 1\n        case false => return 0\n    }\n    return n\n}",
    );
    assert_code(
        "function pick(b: bool) -> int {\n    let n: int = match b {\n        case true => 1\n        case false => return \"zero\"\n    }\n    return n\n}",
        "TPZ5001",
    );
}

// ---- destructuring outside match ------------------------------------------

#[test]
fn let_destructuring_binds_field_types() {
    assert_clean("let { name, age } = { name: \"t\", age: 3 }\nprint(\"{name} {age + 1}\")");
    assert_code(
        "let { name } = { name: \"t\" }\nprint(\"{name + 1}\")",
        "TPZ5001",
    );
}

#[test]
fn for_loops_destructure_their_element() {
    assert_clean(
        "let users: Array<{ name: string }> = [{ name: \"t\" }]\nfor { name } in users {\n    print(name)\n}",
    );
    assert_code(
        "let users: Array<{ name: string }> = [{ name: \"t\" }]\nfor { name } in users {\n    print(\"{name + 1}\")\n}",
        "TPZ5001",
    );
}

#[test]
fn for_loop_type_annotations_check_the_element() {
    assert_code(
        "let xs: Array<int> = [1, 2]\nfor x: string in xs {\n    print(x)\n}",
        "TPZ5001",
    );
    assert_clean("let xs: Array<int> = [1, 2]\nfor x: int in xs {\n    print(\"{x}\")\n}");
}
