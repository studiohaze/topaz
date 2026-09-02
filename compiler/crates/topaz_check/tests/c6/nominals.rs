use super::imports_and_suggestions::STRINGS;
use super::*;

// ---- exported type aliases ---------------------------------------------------

#[test]
fn qualified_types_resolve_through_the_namespace() {
    assert_clean(&[
        ("utils.strings", STRINGS),
        (
            "main",
            "import utils.strings\nlet u: strings.User = { id: 1 }\nprint(\"{u.id}\")",
        ),
    ]);
    assert_code(
        &[
            ("utils.strings", STRINGS),
            (
                "main",
                "import utils.strings\nlet u: strings.User = { id: \"one\" }\nprint(\"{u}\")",
            ),
        ],
        "TPZ5001",
    );
}

#[test]
fn nominal_records_cross_modules_only_through_selected_construction() {
    let clean = check_output_with_version(
        &[
            (
                "main",
                "import model as schema\nlet u: schema.User = schema.make()\nprint(\"{u.name}\")",
            ),
            (
                "model",
                "export record User { name: string = \"Ada\" }\nexport function make() -> User { User {} }",
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        clean.diagnostics.is_empty(),
        "qualified record annotations should resolve: {:?}",
        clean.diagnostics
    );

    let qualified_construct = check_output_with_version(
        &[
            (
                "main",
                "import model as schema\nlet u: schema.User = schema.User { name: \"Ada\" }\nprint(\"{u.name}\")",
            ),
            ("model", "export record User { name: string }"),
        ],
        LangVersion::V5_4,
    );
    assert!(
        qualified_construct.diagnostics.iter().any(|diag| {
            diag.code.as_str() == "TPZ5002"
                && diag.message.contains("is not exported by the module")
        }),
        "namespace-qualified brace construction must reject before backend selection: {:?}",
        qualified_construct.diagnostics
    );

    let selected = check_output_with_version(
        &[
            (
                "main",
                "import model { User }\nlet u: User = User { name: \"Ada\" }\nprint(\"{u.name}\")",
            ),
            ("model", "export record User { name: string }"),
        ],
        LangVersion::V5_4,
    );
    assert!(
        selected.diagnostics.is_empty(),
        "selected record construction should resolve: {:?}",
        selected.diagnostics
    );
}

#[test]
fn exported_receiver_methods_accompany_their_nominal() {
    let clean = check_output_with_version(
        &[
            (
                "main",
                "import model as schema\nlet p: schema.Point = schema.make(4)\nlet x: int = p.coordinate()\nprint(\"{x}\")",
            ),
            (
                "model",
                "export record Point { x: int }\nimpl Point { export function coordinate(self) -> int { self.x } }\nexport function make(x: int) -> Point { Point { x: x } }",
            ),
        ],
        LangVersion::V5_5,
    );
    assert!(
        clean.diagnostics.is_empty(),
        "exported method metadata should follow an exported nominal: {:?}",
        clean.diagnostics
    );

    let private = check_output_with_version(
        &[
            (
                "main",
                "import model as schema\nlet p: schema.Point = schema.make(4)\nlet x = p.coordinate()\nprint(\"{x}\")",
            ),
            (
                "model",
                "export record Point { x: int }\nimpl Point { function coordinate(self) -> int { self.x } }\nexport function make(x: int) -> Point { Point { x: x } }",
            ),
        ],
        LangVersion::V5_5,
    );
    assert!(
        private
            .diagnostics
            .iter()
            .any(|diag| diag.code.as_str() == "TPZ5006"),
        "a private method must not cross the module boundary: {:?}",
        private.diagnostics
    );
}

#[test]
fn same_spelled_exported_receiver_methods_keep_namespace_identity() {
    let output = check_output_with_version(
        &[
            (
                "main",
                "import left\nimport right\nlet a: left.Point = left.make()\nlet b: right.Point = right.make()\nlet x: int = a.coordinate()\nlet y: string = b.coordinate()\nprint(\"{x}{y}\")",
            ),
            (
                "left",
                "export record Point { x: int }\nimpl Point { export function coordinate(self) -> int { self.x } }\nexport function make() -> Point { Point { x: 1 } }",
            ),
            (
                "right",
                "export record Point { x: string }\nimpl Point { export function coordinate(self) -> string { self.x } }\nexport function make() -> Point { Point { x: \"r\" } }",
            ),
        ],
        LangVersion::V5_5,
    );
    assert!(
        output.diagnostics.is_empty(),
        "same-spelled nominals must retain disjoint method metadata: {:?}",
        output.diagnostics
    );
}

#[test]
fn selected_nominal_import_carries_exported_receiver_methods() {
    let output = check_output_with_version(
        &[
            (
                "main",
                "import model { Point as P }\nlet p: P = P { x: 4 }\nlet x: int = p.coordinate()\nprint(\"{x}\")",
            ),
            (
                "model",
                "export record Point { x: int }\nimpl Point { export function coordinate(self) -> int { self.x } }",
            ),
        ],
        LangVersion::V5_5,
    );
    assert!(
        output.diagnostics.is_empty(),
        "selected nominal imports must carry exported methods: {:?}",
        output.diagnostics
    );

    let canonical = typed_output_with_version(
        &[
            (
                "main",
                concat!(
                    "import model { Point as P }\n",
                    "import model as schema\n",
                    "let selected: P = P { x: 1 }.shifted(2)\n",
                    "let selectedPipe: P = 3 |> selected.shifted()\n",
                    "let qualified: schema.Point = schema.make(4).shifted(5)\n",
                    "let qualifiedPipe: schema.Point = 6 |> qualified.shifted()\n",
                ),
            ),
            (
                "model",
                concat!(
                    "export record Point { x: int }\n",
                    "impl Point {\n",
                    "  export function shifted(self, delta: int) -> Point {\n",
                    "    Point { x: self.x + delta }\n",
                    "  }\n",
                    "}\n",
                    "export function make(x: int) -> Point { Point { x: x } }\n",
                ),
            ),
        ],
        LangVersion::V5_20,
    );
    assert!(
        canonical.diagnostics.is_empty(),
        "5.20 selected and namespace receiver methods must keep canonical signatures: {:?}",
        canonical.diagnostics
    );
    assert_eq!(
        canonical
            .typed_hir
            .expect("clean 5.20 receiver method unit")
            .calls
            .into_iter()
            .filter(|call| {
                matches!(
                    &call.plan.callee,
                    topaz_hir::CalleePlan::Member { method, .. }
                        if method == "shifted"
                ) || matches!(
                    &call.plan.callee,
                    topaz_hir::CalleePlan::Pipe {
                        stage_method: Some(method),
                    } if method == "shifted"
                )
            })
            .map(|call| call.target_identity)
            .collect::<Vec<_>>(),
        vec![Some("model::Point".to_string()); 4]
    );
}

#[test]
fn selected_generic_nominal_record_spread_uses_expected_context() {
    let clean = check_output_with_version(
        &[
            (
                "main",
                "import model { Box as Crate }\n\
                 let base: Crate<int> = Crate { value: 7 }\n\
                 let next: Crate<int> = Crate { ...base, }\n\
                 print(\"{next.value}\")",
            ),
            ("model", "export record Box<T> { value: T }"),
        ],
        LangVersion::V5_4,
    );
    assert!(
        clean.diagnostics.is_empty(),
        "selected generic spread should check: {:?}",
        clean.diagnostics
    );
}

#[test]
fn mutable_record_defaults_are_checker_accepted_backend_residuals() {
    let local = check_output_with_version(
        &[(
            "main",
            "let mut base = 36\nrecord User { age: int = base }\nlet u: User = User {}\nprint(\"{u.age}\")",
        )],
        LangVersion::V5_4,
    );
    assert!(
        local.diagnostics.is_empty(),
        "local mutable default is an accepted loud-decline residual: {:?}",
        local.diagnostics
    );

    let imported = check_output_with_version(
        &[
            (
                "main",
                "import model { User }\nlet u: User = User {}\nprint(\"{u.age}\")",
            ),
            (
                "model",
                "let mut base = 36\nexport record User { age: int = base }",
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        imported.diagnostics.is_empty(),
        "imported mutable default is an accepted loud-decline residual: {:?}",
        imported.diagnostics
    );
}

#[test]
fn record_defaults_do_not_bind_self_or_sibling_fields_in_closed_units() {
    for source in [
        "record Bad { first: int = 1, second: int = first }\n0",
        "record Bad { first: int = self.first }\n0",
    ] {
        let output = check_output_with_version(&[("main", source)], LangVersion::V5_4);
        assert!(
            output
                .diagnostics
                .iter()
                .any(|diag| diag.code.as_str() == "TPZ5002"),
            "closed-unit self/sibling default must reject: {:?}",
            output.diagnostics
        );
    }
}

#[test]
fn generic_qualified_aliases_substitute_their_arguments() {
    assert_clean(&[
        ("utils.strings", STRINGS),
        (
            "main",
            "import utils.strings\nlet p: strings.Pair<int> = { a: 1, b: 2 }\nprint(\"{p.a}\")",
        ),
    ]);
    assert_code(
        &[
            ("utils.strings", STRINGS),
            (
                "main",
                "import utils.strings\nlet p: strings.Pair<int> = { a: 1, b: \"two\" }\nprint(\"{p}\")",
            ),
        ],
        "TPZ5001",
    );
}

#[test]
fn selected_type_aliases_bind_directly() {
    assert_clean(&[
        ("utils.strings", STRINGS),
        (
            "main",
            "import utils.strings { User }\nlet u: User = { id: 1 }\nprint(\"{u.id}\")",
        ),
    ]);
}

#[test]
fn selected_record_types_bind_directly() {
    let out = check_output_with_version(
        &[
            (
                "types",
                "export record Box derives Show { value: int }\nexport function make(value: int) -> Box { Box { value: value } }\n",
            ),
            (
                "main",
                "import types { Box, make }\nlet b: Box = make(7)\nprint(\"{b.value}\")",
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        out.diagnostics.is_empty(),
        "expected clean, got: {:?}",
        out.diagnostics
    );
}

#[test]
fn imported_records_form_inside_local_exported_record_fields() {
    let out = check_output_with_version(
        &[
            (
                "main",
                "import model { Row }\n\
                 import transform { Bundle, make }\n\
                 let rows: Array<Row> = [Row { value: 7 }]\n\
                 let bundle: Bundle = make(rows)\n\
                 let roundTrip: Array<Row> = bundle.rows\n\
                 print(\"{roundTrip.length}\")",
            ),
            (
                "transform",
                "import model { Row }\n\
                 export record Bundle { rows: Array<Row> }\n\
                 export function make(rows: Array<Row>) -> Bundle {\n\
                   Bundle { rows: rows }\n\
                 }",
            ),
            ("model", "export record Row { value: int }"),
        ],
        LangVersion::V5_6,
    );
    assert!(
        out.diagnostics.is_empty(),
        "imported record identity must survive a local exported record: {:?}",
        out.diagnostics
    );
}

#[test]
fn v520_selected_nominals_carry_transitive_member_shapes_without_binding_hidden_names() {
    let modules = [
        (
            "main",
            "import model { Outer }\n\
             let outer: Outer = Outer {}\n\
             let value: int = outer.inner.value\n\
             print(\"{value}\")",
        ),
        (
            "model",
            "export record Inner { value: int }\n\
             export record Outer { inner: Inner = Inner { value: 42 } }",
        ),
    ];
    let clean = check_output_with_version(&modules, LangVersion::V5_20);
    assert!(
        clean.diagnostics.is_empty(),
        "a selected nominal must carry the member shapes referenced by its public fields: {:?}",
        clean.diagnostics
    );

    assert_code_at(
        &[
            (
                "main",
                "import model { Outer }\nlet hidden: Inner = Outer {}.inner\nprint(\"{hidden}\")",
            ),
            modules[1],
        ],
        LangVersion::V5_20,
        "TPZ5001",
    );
}

#[test]
fn v520_selected_values_carry_private_nominal_member_shapes() {
    let out = check_output_with_version(
        &[
            (
                "main",
                "import model { make }\nlet value: int = make().value\nprint(\"{value}\")",
            ),
            (
                "model",
                "record Hidden { value: int }\nexport function make() -> Hidden { Hidden { value: 42 } }",
            ),
        ],
        LangVersion::V5_20,
    );
    assert!(
        out.diagnostics.is_empty(),
        "a selected value must carry the private nominal shapes required to use its result: {:?}",
        out.diagnostics
    );
}

#[test]
fn v520_private_nominals_from_distinct_modules_remain_distinct() {
    assert_code_at(
        &[
            (
                "main",
                "import alpha { take }\nimport beta { make }\nlet invalid = take(make())",
            ),
            (
                "alpha",
                "record Hidden { value: int }\nexport function take(value: Hidden) -> int { value.value }",
            ),
            (
                "beta",
                "record Hidden { value: int }\nexport function make() -> Hidden { Hidden { value: 42 } }",
            ),
        ],
        LangVersion::V5_20,
        "TPZ5001",
    );
}

#[test]
fn local_nominals_keep_precedence_over_imported_names() {
    let out = check_output_with_version(
        &[
            (
                "main",
                "import model { Row }\nrecord Row { local: int }\nlet row = Row { local: 1 }",
            ),
            ("model", "export record Row { value: int }"),
        ],
        LangVersion::V5_6,
    );
    assert!(
        out.diagnostics.is_empty(),
        "the established local-over-import precedence must survive module-aware formation: {:?}",
        out.diagnostics
    );
}

#[test]
fn qualified_generic_nominals_substitute_their_arguments() {
    let modules = [
        (
            "types",
            r#"export record Box<T> {
  value: T,
}
export enum Maybe<T> {
  Missing,
  Present(T),
}
export newtype Id<T> = T
export type BoxAlias<T> = Box<T>
export type MaybeAlias<T> = Maybe<T>
export type IdAlias<T> = Id<T>
export function makeBox() -> Box<int> {
  return Box { value: 7 }
}
export function makeMaybe() -> Maybe<int> {
  return Maybe.Present(5)
}
export function makeId() -> Id<string> {
  return Id("u-42")
}
"#,
        ),
        (
            "main",
            r#"import types
let b: types.Box<int> = types.makeBox()
let n: int = b.value
let m: types.Maybe<int> = types.makeMaybe()
let picked: int = match m {
  case Present(x) => x
  case Missing => 0
}
let id: types.Id<string> = types.makeId()
let s: string = id.value()
let aliasedBox: types.BoxAlias<int> = types.makeBox()
let aliasedNumber: int = aliasedBox.value
let aliasedMaybe: types.MaybeAlias<int> = types.makeMaybe()
let aliasedPicked: int = match aliasedMaybe {
  case Present(x) => x
  case Missing => 0
}
let aliasedId: types.IdAlias<string> = types.makeId()
let aliasedString: string = aliasedId.value()
print("{n}:{picked}:{s}:{aliasedNumber}:{aliasedPicked}:{aliasedString}")
"#,
        ),
    ];
    for version in [LangVersion::V5_4, LangVersion::V5_20] {
        let out = check_output_with_version(&modules, version);
        assert!(
            out.diagnostics.is_empty(),
            "expected clean in {version:?}, got: {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn qualified_generic_nominal_collision_uses_qualified_instance_keys() {
    let out = check_output_with_version(
        &[
            (
                "types",
                r#"export record Box<T> {
  value: T,
}
export enum Maybe<T> {
  Missing,
  Present(T),
}
export newtype Id<T> = T
export function makeBox() -> Box<int> {
  return Box { value: 7 }
}
export function makeMaybe() -> Maybe<int> {
  return Maybe.Present(5)
}
export function makeId() -> Id<string> {
  return Id("u-42")
}
"#,
            ),
            (
                "main",
                r#"import types
record Box<T> {
  local: T,
}
enum Maybe<T> {
  Local(T),
}
newtype Id<T> = Array<T>
let b: types.Box<int> = types.makeBox()
let n: int = b.value
let local: Box<int> = Box { local: 9 }
let ln: int = local.local
let m: types.Maybe<int> = types.makeMaybe()
let picked: int = match m {
  case Present(x) => x
  case Missing => 0
}
let id: types.Id<string> = types.makeId()
let s: string = id.value()
print("{n}:{ln}:{picked}:{s}")
"#,
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        out.diagnostics.is_empty(),
        "expected clean, got: {:?}",
        out.diagnostics
    );
}

#[test]
fn qualified_generic_nominal_collision_does_not_expose_local_shape() {
    let out = check_output_with_version(
        &[
            (
                "types",
                "export record Box<T> { value: T }\nexport enum Maybe<T> { Missing, Present(T) }\nexport newtype Id<T> = T\nexport function makeBox() -> Box<int> { Box { value: 7 } }\nexport function makeMaybe() -> Maybe<int> { Maybe.Present(5) }\nexport function makeId() -> Id<string> { Id(\"u-42\") }\n",
            ),
            (
                "main",
                "import types\nrecord Box<T> { local: T }\nenum Maybe<T> { Local(T) }\nnewtype Id<T> = Array<T>\nlet b: types.Box<int> = types.makeBox()\nlet badField = b.local\nlet m: types.Maybe<int> = types.makeMaybe()\nlet badVariant = match m {\n  case Local(x) => x\n  case _ => 0\n}\nlet id: types.Id<string> = types.makeId()\nlet badValue: Array<string> = id.value()\n",
            ),
        ],
        LangVersion::V5_4,
    );
    let codes: Vec<&str> = out.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"TPZ5006"),
        "expected unknown local field rejection, got: {:?}",
        out.diagnostics
    );
    assert!(
        codes.contains(&"TPZ5001"),
        "expected local variant/newtype mismatch rejection, got: {:?}",
        out.diagnostics
    );
}

#[test]
fn selected_generic_nominals_bind_directly() {
    let out = check_output_with_version(
        &[
            (
                "types",
                "export record Box<T> { value: T }\nexport function makeBox() -> Box<int> {\n  return Box { value: 7 }\n}\n",
            ),
            (
                "main",
                "import types { Box, makeBox }\nlet b: Box<int> = makeBox()\nlet n: int = b.value\nprint(\"{n}\")\n",
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        out.diagnostics.is_empty(),
        "expected clean, got: {:?}",
        out.diagnostics
    );
}

#[test]
fn selected_newtype_alias_works_as_constructor_pattern_and_type() {
    let out = check_output_with_version(
        &[
            (
                "main",
                "import model { UserId as Uid }\nlet id: Uid = Uid(7)\nlet n: int = match id {\n  case Uid(value) => value\n}\nprint(\"{n}\")\n",
            ),
            ("model", "export newtype UserId = int\n"),
        ],
        LangVersion::V5_4,
    );
    assert!(
        out.diagnostics.is_empty(),
        "selected alias should work in all three positions: {:?}",
        out.diagnostics
    );
}

#[test]
fn selected_nominal_aliases_rebind_imported_value_signatures() {
    for version in [LangVersion::V5_5, LangVersion::V5_6, LangVersion::V5_7] {
        let out = check_output_with_version(
            &[
                (
                    "main",
                    "import model { Msg as M, User as U, Id as I, makeMsg, makeUser, makeId }\nlet msg: M = makeMsg()\nlet user: U = makeUser()\nlet id: I = makeId(7)\nprint(\"{msg}/{user.name}/{id.value()}\")\n",
                ),
                (
                    "model",
                    "export enum Msg { Ready }\nexport record User { name: string }\nexport newtype Id = int\nexport function makeMsg() -> Msg { Msg.Ready }\nexport function makeUser() -> User { User { name: \"Ada\" } }\nexport function makeId(value: int) -> Id { Id(value) }\n",
                ),
            ],
            version,
        );
        assert!(
            out.diagnostics.is_empty(),
            "selected nominal aliases must rewrite imported value signatures in {version:?}: {:?}",
            out.diagnostics
        );
    }
}

#[test]
fn imported_derived_conformances_follow_local_nominal_identity() {
    for version in [LangVersion::V5_19, LangVersion::V5_20] {
        let accepted = check_output_with_version(
            &[
                (
                    "main",
                    "import alpha as Other\nimport beta { User as ImportedUser, make }\nfunction label<T: Show>(value: T) -> string { Show.show(value) }\nlet user: ImportedUser = make()\nlet text: string = label(user)\n",
                ),
                (
                    "alpha",
                    "export record User derives Eq { id: int }\nexport function make() -> User { User { id: 1 } }\n",
                ),
                (
                    "beta",
                    "export record User derives Show { name: string }\nexport function make() -> User { User { name: \"Ada\" } }\n",
                ),
            ],
            version,
        );
        assert!(
            accepted.diagnostics.is_empty(),
            "selected imported conformance must follow its nominal in {version:?}: {:?}",
            accepted.diagnostics
        );

        let rejected = check_output_with_version(
            &[
                (
                    "main",
                    "import alpha as Other\nimport beta { User as ImportedUser }\nfunction label<T: Show>(value: T) -> string { Show.show(value) }\nlet text: string = label(Other.make())\n",
                ),
                (
                    "alpha",
                    "export record User derives Eq { id: int }\nexport function make() -> User { User { id: 1 } }\n",
                ),
                ("beta", "export record User derives Show { name: string }\n"),
            ],
            version,
        );
        assert_eq!(
            rejected
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code.as_str() == "TPZ5522")
                .count(),
            1,
            "a same-spelled conformance from another module must not leak in {version:?}: {:?}",
            rejected.diagnostics
        );
    }
}

#[test]
fn differently_shaped_same_source_newtypes_do_not_unify_statically() {
    let out = check_output_with_version(
        &[
            (
                "main",
                "import ints { Token as IntToken, make as makeInt }\nimport texts { Token as TextToken, make as makeText }\nlet a: IntToken = makeInt()\nlet b: TextToken = makeText()\nlet bad: IntToken = b\n",
            ),
            (
                "ints",
                "export newtype Token = int\nexport function make() -> Token { Token(1) }\n",
            ),
            (
                "texts",
                "export newtype Token = string\nexport function make() -> Token { Token(\"x\") }\n",
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|diag| diag.code.as_str() == "TPZ5001"),
        "same-spelled non-equivalent imported newtypes must remain distinct: {:?}",
        out.diagnostics
    );
}

#[test]
fn selected_and_qualified_equivalent_generic_nominals_share_the_bare_instance() {
    let out = check_output_with_version(
        &[
            (
                "types",
                "export record Box<T> { value: T }\nexport function makeBox() -> Box<int> {\n  return Box { value: 7 }\n}\n",
            ),
            (
                "main",
                "import types\nimport types { Box, makeBox }\nlet selected: Box<int> = makeBox()\nlet s: int = selected.value\nlet qualified: types.Box<int> = types.makeBox()\nlet q: int = qualified.value\nprint(\"{s}:{q}\")\n",
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        out.diagnostics.is_empty(),
        "expected clean, got: {:?}",
        out.diagnostics
    );
}

#[test]
fn qualified_generic_nominal_arity_errors_are_reported() {
    let out = check_output_with_version(
        &[
            (
                "types",
                "export record Box<T> { value: T }\nexport function makeBox() -> Box<int> {\n  return Box { value: 7 }\n}\n",
            ),
            (
                "main",
                "import types\nlet b: types.Box<int, string> = types.makeBox()\nprint(\"{b}\")\n",
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "TPZ5022"),
        "expected TPZ5022, got: {:?}",
        out.diagnostics
    );
}

#[test]
fn qualified_generic_nominal_mismatches_are_reported() {
    let out = check_output_with_version(
        &[
            (
                "types",
                "export record Box<T> { value: T }\nexport function makeBox() -> Box<int> {\n  return Box { value: 7 }\n}\n",
            ),
            (
                "main",
                "import types\nlet b: types.Box<string> = types.makeBox()\nprint(\"{b}\")\n",
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        out.diagnostics.iter().any(|d| d.code.as_str() == "TPZ5001"),
        "expected TPZ5001, got: {:?}",
        out.diagnostics
    );
}

#[test]
fn unknown_qualified_types_are_tpz5025() {
    assert_code(
        &[
            ("utils.strings", STRINGS),
            (
                "main",
                "import utils.strings\nlet u: strings.Nope = { id: 1 }\nprint(\"{u}\")",
            ),
        ],
        "TPZ5025",
    );
    assert_code(
        &[("main", "let u: nowhere.User = { id: 1 }\nprint(\"{u}\")")],
        "TPZ5025",
    );
}
