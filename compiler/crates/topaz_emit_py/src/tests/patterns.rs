use super::*;

#[test]
fn emits_wildcard_and_literal_match_patterns() {
    let generated = emit_source(
        r#"
function main() -> string {
    let label = match "ko" {
        case "en" => "English"
        case "ko" => "Korean"
        case _ => "Other"
    }
    match 7 {
        case 8 => print("bad")
        case _ => print(label)
    }
    label
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_eq(__tpz_match, \"ko\", "),
        "{generated}"
    );
    assert!(generated.contains("elif True:"), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("wildcard/literal match Python gate failed: {e}"));
}

#[test]
fn emits_constructor_and_nominal_field_subpatterns() {
    let generated = emit_source(
        r#"
record User { name: string, age: int }
function main() -> int {
    let nested = Ok(Ok(7))
    let n = match nested {
        case Ok(Ok(value)) => value
        case _ => 0
    }
    let u = User { name: "Ada", age: n }
    match u {
        case User { age: 7 } => n
        case _ => 0
    }
}
main()
"#,
    );
    assert!(
        generated.contains("isinstance(__tpz_match, Ok)"),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_eq(__tpz_match._t_616765, 7, "),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("constructor subpattern Python gate failed: {e}"));
}

#[test]
fn emits_structural_record_and_list_patterns() {
    let generated = emit_source(
        r#"
function main() -> int {
    let r = { x: 1, nested: { y: 2 } }
    let a = match r {
        case { x, nested: { y: 2 } } => x
        case _ => 0
    }
    let xs = [1, 2, 3, 4]
    let b = match xs {
        case [1, head, ..tail] => head + tail[0]
        case _ => 0
    }
    a + b
}
main()
"#,
    );
    assert!(generated.contains("__topaz_record_fields__"), "{generated}");
    assert!(
        generated.contains("isinstance(__tpz_match, list)"),
        "{generated}"
    );
    assert!(generated.contains("[2:]"), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("structural/list pattern Python gate failed: {e}"));
}

#[test]
fn emits_destructuring_let_through_pattern_guard() {
    let generated = emit_source(
        r#"
function makeRecord() {
    print("record")
    { x: 1, nested: { y: 2 } }
}
function main() -> string {
    let { x, nested: { y } } = makeRecord()
    let [head, ..tail] = [3, 4, 5]
    "{x}:{y}:{head}:{tail}"
}
main()
"#,
    );
    assert!(generated.contains("tpz_let_pattern("), "{generated}");
    assert!(
        generated.contains("tpz_record_field(") && generated.contains("[1:]"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("destructuring let Python gate failed: {e}"));
}

#[test]
fn destructuring_let_guard_behavior_matches_local_entry_and_imported_scopes() {
    let local = emit_source(
        r#"
function main() -> int {
    let [value] = []
    value
}
main()
"#,
    );
    assert_generated_python_fault_code(&local, "TPZ5001", "local destructuring let guard");

    let entry = emit_source("let [value] = []\nvalue");
    assert_generated_python_fault_code(&entry, "TPZ5001", "entry destructuring let guard");

    let imported = emit_source_with_files(
        "import model { out }\nout",
        &[("model.tpz", "let [value] = []\nexport let out = value")],
    );
    assert_generated_python_fault_code(&imported, "TPZ5001", "imported destructuring let guard");

    let imported_value = emit_source_with_files(
        "import model { out }\nout",
        &[("model.tpz", "let [value] = [7]\nexport let out = value")],
    );
    assert_generated_python_ok_int(
        &imported_value,
        7,
        "imported destructuring let binding projection",
    );
}

#[test]
fn emits_range_match_patterns_without_bool_leakage() {
    let generated = emit_source(
        r#"
function main() -> int {
    let a = match 7 {
        case 1..9 => 1
        case _ => 0
    }
    let b = match 9 {
        case 1..<9 => 100
        case _ => 2
    }
    let c = match true {
        case 0..1 => 1000
        case _ => 3
    }
    a + b + c
}
main()
"#,
    );
    assert!(
        generated.contains("type(__tpz_match) is int"),
        "{generated}"
    );
    assert!(generated.contains("__tpz_match <= 9"), "{generated}");
    assert!(generated.contains("__tpz_match < 9"), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("range pattern Python gate failed: {e}"));
}

#[test]
fn emits_range_expressions_for_iteration_and_render() {
    let generated = emit_source(
        r#"
function main() -> string {
    let mut total = 0
    for x in 1..4 {
        total = total + x
    }
    let mut stepped = 0
    for x in 0..<10 by 2 {
        stepped = stepped + x
    }
    "{total}:{stepped}:{5..1 by -2}"
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_range(1, 4, True, None,"),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_range(0, 10, False, 2,"),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_range(5, 1, True, tpz_neg(2,"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("range expression Python gate failed: {e}"));
}

#[test]
fn emits_membership_operator_through_topaz_helper() {
    let generated = emit_source(
        r#"
function main() -> string {
    let a = 2 in [1, 2, 3]
    let b = "z" in ["a"]
    let c = 2 in (1..4)
    let s = Set.of("a", "b")
    let d = "b" in s
    "{a}:{b}:{c}:{d}"
}
main()
"#,
    );
    assert!(generated.contains("tpz_in(2, [1, 2, 3],"), "{generated}");
    assert!(generated.contains("tpz_in(\"z\", [\"a\"],"), "{generated}");
    assert!(
        generated.contains("tpz_in(2, (tpz_range(1, 4, True, None,"),
        "{generated}"
    );
    assert!(generated.contains("tpz_in(\"b\", _t_73,"), "{generated}");
    assert!(!generated.contains(" in ["), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("membership Python gate failed: {e}"));
}

#[test]
fn emits_core_pipe_expressions() {
    let generated = emit_source(
        r#"
function inc(x: int) -> int {
    x + 1
}
function add(a: int, b: int) -> int {
    a + b
}
	function sub(a: int, b: int) -> int {
	    a - b
	}
	function label(prefix: string, value: string) -> string {
	    "{prefix}:{value}"
	}
	function main() {
	    let localSub = (a: int, b: int) => a - b
	    let r = { x: 5 }
	    {
	        unary: 5 |> inc,
	        withArg: 5 |> add(3),
	        field: r |> .x,
	        placeholder: 3 |> localSub(10, _),
	        nestedPlaceholder: 1 |> add(10 + _),
	        repeatedPlaceholder: 3 |> add(_, _ + 4),
	        namedInsert: 10 |> sub(b: 3),
	        namedPlaceholder: 10 |> sub(b: 3, a: _),
	        namedNestedPlaceholder: 1 |> add(b: 10 + _, a: 2),
	        nestedPipeScope: 5 |> add(_, 1 + (2 |> add(_, 3))),
	        nestedPipeLhsScope: 7 |> add(_, (_ |> add(_, 1))),
	        interpolatedPlaceholder: "hi" |> label("value {_}", _),
	        localNamed: 10 |> localSub(b: 4, a: _)
	    }
	}
	main()
	"#,
    );
    assert!(
        generated.contains("(lambda __tpz_piped: _t_696e63(host, __tpz_piped))(5)"),
        "{generated}"
    );
    assert!(
        generated.contains("(lambda __tpz_piped: _t_616464(host, __tpz_piped, 3))(5)"),
        "{generated}"
    );
    assert!(
        generated.contains("(lambda __tpz_piped: tpz_member(__tpz_piped, \"_t_78\", \"x\","),
        "{generated}"
    );
    assert!(
        generated.contains("(lambda __tpz_piped: _t_6c6f63616c537562(10, __tpz_piped))(3)"),
        "{generated}"
    );
    assert!(
        generated.contains("tpz_call_order_fault([tpz_add(10, __tpz_piped,")
            && generated.contains("missing argument for parameter `b`"),
        "nested pipe placeholder with a missing slot should lower to the shared call-order fault: {generated}"
    );
    assert!(
        generated.contains("_t_616464(host, __tpz_piped, tpz_add(__tpz_piped, 4,"),
        "repeated pipe placeholders should reuse the same single-evaluated piped value: {generated}"
    );
    assert!(
        generated.contains("_t_737562(host, __tpz_piped, _t_62=3"),
        "named pipe stage without placeholder should keep first-arg insertion: {generated}"
    );
    assert!(
        generated.contains("_t_737562(host, _t_62=3, _t_61=__tpz_piped"),
        "named pipe stage placeholder should bind the named slot: {generated}"
    );
    assert!(
        generated.contains("_t_616464(host, _t_62=tpz_add(10, __tpz_piped,"),
        "named nested pipe placeholder should bind inside the named slot: {generated}"
    );
    assert!(
	            generated.contains("_t_616464(host, __tpz_piped, tpz_add(1, ((lambda __tpz_piped: _t_616464(host, __tpz_piped, 3))(2))"),
	            "nested pipe scopes should keep independent placeholder bindings: {generated}"
	        );
    assert!(
	            generated.contains("_t_616464(host, __tpz_piped, ((lambda __tpz_piped: _t_616464(host, __tpz_piped, 1))(__tpz_piped)))"),
	            "nested pipe lhs placeholders should feed the inner pipe without losing the outer binding: {generated}"
	        );
    assert!(
        generated.contains("''.join([\"value \", tpz_render(__tpz_piped)])"),
        "string interpolation should substitute pipe placeholders inside expression slots: {generated}"
    );
    assert!(
        generated.contains("_t_6c6f63616c537562(_t_62=4, _t_61=__tpz_piped"),
        "local lambda named pipe stage should bind by local lambda params: {generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("core pipe expression Python gate failed: {e}"));
}

#[test]
fn emits_or_match_patterns_with_first_match_bindings() {
    let generated = emit_source(
        r#"
function main() -> int {
    let a = match 2 {
        case 1 | 2 => 3
        case _ => 0
    }
    let b = match 5 {
        case 1..<3 | 5 => 4
        case _ => 0
    }
    let c = match [1, 2] {
        case [x, 2] | [0, x] => x
        case _ => 0
    }
    let d = match [0, 7] {
        case [x, 2] | [0, x] => x
        case _ => 0
    }
    a + b + c + d
}
main()
"#,
    );
    assert!(generated.contains(" or "), "{generated}");
    assert!(
        generated.contains("if (isinstance(__tpz_match, list)"),
        "{generated}"
    );
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("or pattern Python gate failed: {e}"));
}

#[test]
fn emits_typed_match_patterns_for_scalar_and_structural_shapes() {
    let generated = emit_source(
        r#"
function main() -> int {
    let a = match 5 {
        case n: int => n
        case _ => 0
    }
    let b = match "x" {
        case s: string => 2
        case _ => 0
    }
    let c = match true {
        case n: int => 100
        case flag: bool => 3
        case _ => 0
    }
    let d = match [4].get(0) {
        case opt: Option<int> => 4
        case _ => 0
    }
    let e = match Ok("x") {
        case res: Result<int, string> => 5
        case _ => 0
    }
    let f = match [1, 2] {
        case xs: Array<int> => 6
        case _ => 0
    }
    let g = match { x: 1 } {
        case rec: { x: int } => 7
        case _ => 0
    }
    a + b + c + d + e + f + g
}
main()
"#,
    );
    assert!(generated.contains("tpz_type_matches"), "{generated}");
    assert!(generated.contains("\"record\""), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("typed pattern Python gate failed: {e}"));
}

#[test]
fn emits_literal_typed_match_patterns_with_interpreter_text_semantics() {
    let generated = emit_source(
        r#"
function main() -> int {
    let int_or_float_a: int | float = 7.0
    let a = match int_or_float_a {
        case n: 7 => 10
        case _ => 0
    }
    let int_or_float_b: int | float = 7
    let b = match int_or_float_b {
        case n: 7 => 11
        case _ => 0
    }
    let c = match 7 {
        case n: 7.0 => 100
        case _ => 3
    }
    let d = match 7.0 {
        case n: 7.0 => 4
        case _ => 0
    }
    let slash_t = "a\\tb"
    let e = match slash_t {
        case s: "a\tb" => 5
        case _ => 0
    }
    let tab = "a\tb"
    let f = match tab {
        case s: "a\tb" => 100
        case _ => 6
    }
    let g = match true {
        case flag: true => 7
        case _ => 0
    }
    let h = match null {
        case value: null => 8
        case _ => 0
    }
    a + b + c + d + e + f + g + h
}
main()
"#,
    );
    assert!(generated.contains("(\"literal\", \"7\")"), "{generated}");
    assert!(
        generated.contains("(\"literal\", \"\\\"a\\\\tb\\\"\")"),
        "{generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        54,
        "literal typed pattern interpreter text semantics",
    );
}

#[test]
fn emits_nominal_typed_match_patterns_for_local_and_imported_records() {
    let generated = emit_source(
        r#"
record User { name: string, age: int }
record Admin { name: string, age: int }
function main() -> int {
    let u = User { name: "Ada", age: 36 }
    let a = match u {
        case person: User if person.age > 30 => person.age
        case _ => 0
    }
    let b = match u {
        case admin: Admin => 100
        case _ => 1
    }
    let c = match { name: "Ada", age: 36 } {
        case person: User => 1000
        case _ => 2
    }
    let d = match u {
        case person: User | Admin => person.age
        case _ => 0
    }
    a + b + c + d
}
main()
"#,
    );
    assert!(
        generated.contains("(\"nominal_record\", \"User\")"),
        "{generated}"
    );
    assert!(
        generated.contains("(\"nominal_record\", \"Admin\")"),
        "{generated}"
    );
    assert!(generated.contains("tpz_type_matches"), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("local nominal typed pattern Python gate failed: {e}"));

    let imported = emit_source_with_files(
        r#"
import alias_model { User as Person, Admin }
import model
function main() -> string {
    let p = Person { name: "Ada", age: 36 }
    let q = Admin { name: "Root", age: 40 }
    let m = model.make()
    let selected = match p {
        case person: Person if person.age > 30 => "{person.name}:{person.age}"
        case _ => "no"
    }
    let qualified = match m {
        case user: model.User => "{user.name}:{user.age}"
        case _ => "no"
    }
    let negative = match q {
        case user: Person => "bad"
        case _ => "ok"
    }
    let crossModule = match p {
        case user: model.User => "same-id"
        case _ => "ok"
    }
    "{selected}:{qualified}:{negative}:{crossModule}"
}
main()
"#,
        &[
            (
                "alias_model.tpz",
                r#"
export record User { name: string, age: int }
export record Admin { name: string, age: int }
"#,
            ),
            (
                "model.tpz",
                r#"
export record User { name: string, age: int }
export function make() -> User {
    User { name: "Grace", age: 41 }
}
"#,
            ),
        ],
    );
    assert!(
        imported.contains("(\"nominal_record\", \"User\")"),
        "{imported}"
    );
    assert_generated_python_gates(&imported)
        .unwrap_or_else(|e| panic!("imported nominal typed pattern Python gate failed: {e}"));
}

#[test]
fn emits_same_module_newtype_construct_unwrap_pattern_and_type_guard() {
    let generated = emit_source(
        r#"
newtype UserId = int

function main() -> int {
    let id: UserId = UserId(41)
    let direct = id.value()
    let matched = match id {
        case UserId(x) => x + 1
    }
    let typed = match id {
        case found: UserId => found.value() + 2
        case _ => 0
    }
    direct + matched + typed
}

main()
"#,
    );
    assert!(
        generated.contains("tpz_newtype(\"UserId\", 41"),
        "newtype constructor should lower through the runtime wrapper: {generated}"
    );
    assert!(
        generated.contains("tpz_newtype_unwrap("),
        "newtype .value() should lower through the runtime unwrap helper: {generated}"
    );
    assert!(
        generated.contains("tpz_is_newtype(__tpz_match"),
        "newtype constructor patterns should test the nominal id: {generated}"
    );
    assert!(
        generated.contains("(\"newtype\", \"UserId\")"),
        "typed newtype guards should use nominal type specs: {generated}"
    );
    assert_generated_python_ok_int(&generated, 126, "same-module newtype parity");
}

#[test]
fn emits_imported_and_generic_newtype_constructors_with_source_ids() {
    let imported = emit_source_with_files(
        r#"
import model { UserId as Uid }

function main() -> int {
    let id: Uid = Uid(41)
    let direct = id.value()
    let matched = match id {
        case Uid(x) => x + 1
    }
    let typed = match id {
        case found: Uid => found.value() + 2
        case _ => 0
    }
    direct + matched + typed
}

main()
"#,
        &[(
            "model.tpz",
            r#"
export newtype UserId = int
"#,
        )],
    );
    assert!(
        imported.contains("tpz_newtype(\"UserId\", 41"),
        "selected imported newtype constructors should keep the defining source id: {imported}"
    );
    assert!(
        imported.contains("(\"newtype\", \"UserId\")"),
        "selected imported newtype typed guards should keep the defining source id: {imported}"
    );
    assert_generated_python_ok_int(&imported, 126, "selected imported newtype parity");

    let imported_generic = emit_source_with_files(
        r#"
import model { Box as ForeignBox }

function main() -> int {
    let b = ForeignBox(7)
    let matched = match b {
        case ForeignBox(value) => value + 1
    }
    let typed = match b {
        case found: ForeignBox<int> => found.value() + 2
        case _ => 0
    }
    b.value() + matched + typed
}

main()
"#,
        &[(
            "model.tpz",
            r#"
export newtype Box<T> = T
"#,
        )],
    );
    assert!(
        imported_generic.contains("tpz_newtype(\"Box\", 7"),
        "imported generic newtype constructors should erase type params but keep the defining source id: {imported_generic}"
    );
    assert!(
        imported_generic.contains("tpz_is_newtype(__tpz_match")
            && imported_generic.contains("\"Box\""),
        "imported generic newtype constructor patterns should test the erased source id: {imported_generic}"
    );
    assert_generated_python_ok_int(&imported_generic, 24, "imported generic newtype parity");
}

#[test]
fn generic_newtype_typed_patterns_validate_substituted_bases() {
    let generated = emit_source(
        r#"
newtype Box<T> = T
function main() -> int {
    let value: Box<int> = Box(7)
    match value {
        case found: Box<int> => found.value()
        case _ => 0
    }
}
main()
"#,
    );
    assert!(
        generated.contains("newtype:Box<\\\"int\\\">") && generated.contains("\"int\""),
        "generic typed patterns must carry the substituted base spec: {generated}"
    );
    assert_generated_python_ok_int(&generated, 7, "generic newtype typed pattern");

    let wrong_base = emit_unchecked_source(
        r#"
newtype Box<T> = T
let value = Box("wrong")
match value {
    case found: Box<int> => 1
    case _ => 0
}
"#,
    );
    assert_generated_python_ok_int(
        &wrong_base,
        0,
        "generic newtype typed pattern wrong-base fallthrough",
    );

    let recursive = emit_source(
        r#"
newtype Link<T> = Option<Link<T>>
let value: Link<int> = Link(None)
match value {
    case found: Link<int> => 1
    case _ => 0
}
"#,
    );
    assert!(
        recursive.contains("type_ref") && recursive.contains("newtype:Link<\\\"int\\\">"),
        "recursive newtype patterns must use bounded type references: {recursive}"
    );
    assert_generated_python_ok_int(&recursive, 1, "recursive generic newtype pattern");
}

#[test]
fn newtype_json_stringify_is_transparent_through_nested_bases() {
    let generated = emit_source(
        r#"
newtype UserId = int
newtype Email = string
newtype Box<T> = T
newtype Outer<T> = Box<T>
function main() -> string {
    let boxed: Box<int> = Box(8)
    let outer: Outer<int> = Outer(boxed)
    match JSON.stringify([UserId(7), Email("a@b.c"), outer]) {
        case Ok(text) => text
        case Err(error) => error
    }
}
main()
"#,
    );
    assert_generated_python_ok_string(&generated, "[7,\"a@b.c\",8]", "transparent newtype JSON");
}

#[test]
fn newtype_first_class_value_bridge_captures_and_unwraps_one_layer() {
    let generated = emit_source(
        r#"
newtype UserId = int
newtype Inner = int
newtype Outer = Inner
function main() -> int {
    let mut acquisitions = 0
    function acquire() -> UserId {
        acquisitions = acquisitions + 1
        UserId(7)
    }
    let get: () -> int = acquire().value
    let callbacks: Array<() -> int> = [get]
    let outer: Outer = Outer(Inner(9))
    let first: () -> Inner = outer.value
    let scheduled = concurrent {
        use: get()
        idle: 0
    }
    acquisitions * 100 + get() + callbacks[0]() + first().value() + scheduled.use
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_member(")
            && generated.contains("\"value\"")
            && generated.contains("tpz_newtype_unwrap("),
        "bare value members must become bound one-layer unwrap callables:\n{generated}"
    );
    assert_generated_python_ok_int(&generated, 130, "first-class newtype value bridge");
}

#[test]
fn emits_imported_monomorphic_enum_constructors_patterns_and_type_guards() {
    let generated = emit_source_with_files(
        r#"
import model { Status as RemoteStatus }
import qualified

function main() -> int {
    let ready = RemoteStatus.Ready
    let busy = RemoteStatus.Busy(7)
    let a = match ready {
        case Ready => 1
        case Busy(_) => 100
    }
    let b = match busy {
        case Busy(n) => n
        case Ready => 0
    }
    let c = match busy {
        case found: RemoteStatus => if found == RemoteStatus.Busy(7) { 3 } else { 0 }
    }
    let d = match qualified.make() {
        case found: qualified.Mode => 4
        case _ => 0
    }
    let mut labels = Map.new()
    labels.insert(RemoteStatus.Busy(7), 5)
    let statuses = set { ready, RemoteStatus.Ready, busy }
    a + b + c + d + labels.getOr(busy, 0) + statuses.length
}

main()
"#,
        &[
            (
                "model.tpz",
                r#"
export enum Status derives Eq, Order, Show { Ready, Busy(int) }
"#,
            ),
            (
                "qualified.tpz",
                r#"
export enum Mode derives Eq, Order, Show { Ready, Busy(int) }
export function make() -> Mode {
    Mode.Busy(1)
}
"#,
            ),
        ],
    );
    assert!(
        generated.contains("tpz_enum(\"Status\", \"Ready\""),
        "selected imported enum payloadless variants should keep the defining source id: {generated}"
    );
    assert!(
        generated.contains("tpz_enum(\"Status\", \"Busy\""),
        "selected imported enum payload variants should keep the defining source id: {generated}"
    );
    assert!(
        generated.contains("(\"enum\", \"Status\")") && generated.contains("(\"enum\", \"Mode\")"),
        "selected and namespace-qualified imported enum typed guards should use defining source ids: {generated}"
    );
    assert_generated_python_ok_int(&generated, 22, "imported monomorphic enum parity");
}

#[test]
fn emits_generic_and_namespace_imported_enum_runtime_ops() {
    let generic = emit_source(
        r#"
enum Box<T> derives Eq, Order, Show { Empty, One(T), Two(T, T) }
function main() -> int {
    let empty: Box<int> = Box.Empty
    let one: Box<int> = Box.One(7)
    let two: Box<int> = Box.Two(2, 3)
    let a = match one {
        case One(value) => value
        case Empty => 0
        case Two(left, right) => left + right
    }
    let b = match two {
        case found: Box<int> => if found == Box.Two(2, 3) { 4 } else { 0 }
    }
    let c = match Box.One("x") {
        case found: Box<int> => 100
        case _ => 5
    }
    let mut labels = Map.new()
    labels.insert(one, 6)
    let boxes = set { empty, Box.Empty, one, two }
    a + b + c + labels.getOr(Box.One(7), 0) + boxes.length
}

main()
"#,
    );
    assert!(
        generic.contains("(\"enum\", \"Box\","),
        "generic enum typed guards should carry payload specs: {generic}"
    );
    assert!(
        generic.contains("(\"One\", (\"int\",))")
            && generic.contains("(\"Two\", (\"int\", \"int\"))"),
        "generic enum payload specs should instantiate type parameters: {generic}"
    );
    assert_generated_python_ok_int(&generic, 25, "same-module generic enum parity");

    let recursive = emit_source(
        r#"
enum List<T> derives Eq, Order, Show { Nil, Cons(T, List<T>) }
function sum(xs: List<int>) -> int {
    match xs {
        case Nil => 0
        case Cons(head, tail) => head + sum(tail)
    }
}
function main() -> int {
    let xs: List<int> = List.Cons(1, List.Cons(2, List.Cons(3, List.Nil)))
    match xs {
        case found: List<int> => sum(found)
        case _ => 0
    }
}

main()
"#,
    );
    assert!(
        recursive.contains("\"type_ref\"")
            && recursive.contains("enum:List")
            && recursive.contains("(\"Cons\", (\"int\", (\"type_ref\""),
        "recursive generic enum specs should close over a type_ref instead of declining or expanding forever: {recursive}"
    );
    assert_generated_python_ok_int(&recursive, 6, "recursive generic enum parity");

    let namespace = emit_source_with_files(
        r#"
import model
function main() -> int {
    let ready = model.ready()
    let busy = model.busy(3)
    let a = match busy {
        case Busy(n) => n
        case Ready => 0
    }
    let b = match ready {
        case found: model.Status => if found == model.ready() { 4 } else { 0 }
        case _ => 0
    }
    let mut labels = Map.new()
    labels.insert(busy, 5)
    let statuses = set { ready, model.ready(), busy }
    a + b + labels.getOr(model.busy(3), 0) + statuses.length
}

main()
"#,
        &[(
            "model.tpz",
            r#"
export enum Status derives Eq, Order, Show { Ready, Busy(int) }
export function ready() -> Status {
    Status.Ready
}
export function busy(n: int) -> Status {
    Status.Busy(n)
}
"#,
        )],
    );
    assert!(
        namespace.contains("tpz_enum_pattern(") && namespace.contains("\"Status\""),
        "namespace-imported enum values should be visible to bare variant patterns: {namespace}"
    );
    assert_generated_python_ok_int(&namespace, 14, "namespace imported enum value parity");
}

#[test]
fn emits_enum_bare_variant_non_owner_fallback_binding() {
    let generated = emit_source(
        r#"
enum Color derives Eq, Order, Show { Red, Blue }

function main() -> int {
    match 7 {
        case Red => return Red + 1
        case _ => return 0
    }
    0
}

main()
"#,
    );
    assert!(
        generated.contains("tpz_enum_bare_variant_matches("),
        "bare enum variant binding should use the non-owner fallback matcher: {generated}"
    );
    assert!(
        generated.contains("tpz_enum_bare_variant_binds("),
        "bare enum variant binding should gate the fallback binding assignment: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        8,
        "same-module enum bare variant fallback binding",
    );
}

#[test]
fn emits_generic_nominal_typed_match_patterns() {
    let local = emit_source(
        r#"
record Box<T> { value: T }
function main() -> int {
    let b = Box { value: 7 }
    match b {
        case found: Box<int> => found.value
        case _ => 0
    }
}
main()
"#,
    );
    assert!(local.contains("(\"nominal_record\", \"Box\","), "{local}");
    assert!(local.contains("\"int\""), "{local}");
    assert!(local.contains("tpz_type_matches("), "{local}");
    assert_generated_python_gates(&local)
        .unwrap_or_else(|e| panic!("local generic nominal typed pattern failed: {e}"));

    let imported = emit_source_with_files(
        r#"
import selected_model { Box as ForeignBox, makeIntBox, makeStringBox }
import qualified_model
function main() -> string {
    let b = makeIntBox(7)
    let c = makeStringBox("x")
    let selected = match b {
        case found: ForeignBox<int> => "{found.value}"
        case _ => "bad"
    }
    let q = qualified_model.makeIntBox(9)
    let qualified = match q {
        case found: qualified_model.Box<int> => "{found.value}"
        case _ => "bad"
    }
    let mismatch = match c {
        case found: ForeignBox<int> => "bad"
        case _ => "ok"
    }
    "{selected}:{qualified}:{mismatch}"
}
main()
"#,
        &[
            (
                "selected_model.tpz",
                r#"
export record Box<T> { value: T }
export function makeIntBox(value: int) -> Box<int> {
    Box { value: value }
}
export function makeStringBox(value: string) -> Box<string> {
    Box { value: value }
}
"#,
            ),
            (
                "qualified_model.tpz",
                r#"
export record Box<T> { value: T }
export function makeIntBox(value: int) -> Box<int> {
    Box { value: value }
}
"#,
            ),
        ],
    );
    assert!(
        imported.contains("(\"nominal_record\", \"Box\","),
        "{imported}"
    );
    assert_generated_python_gates(&imported)
        .unwrap_or_else(|e| panic!("imported generic nominal typed pattern failed: {e}"));

    let nested = emit_source(
        r#"
record Box<T> { value: T }
function main() -> int {
    let inner = Box { value: 7 }
    let outer = Box { value: inner }
    let arrays = Box { value: [1, 2] }
    let optional = Box { value: Some(3) }
    let a = match outer {
        case found: Box<Box<int>> => found.value.value
        case _ => 0
    }
    let b = match arrays {
        case found: Box<Array<int>> => found.value.length
        case _ => 0
    }
    let c = match optional {
        case found: Box<Option<int>> => 3
        case _ => 0
    }
    a + b + c
}
main()
"#,
    );
    assert!(nested.contains("\"array\""), "{nested}");
    assert!(nested.contains("\"option\""), "{nested}");
    assert_generated_python_gates(&nested)
        .unwrap_or_else(|e| panic!("nested generic nominal typed pattern failed: {e}"));

    let phantom = emit_source(
        r#"
record Tag<T> { label: string }
function main() -> string {
    let tag = Tag { label: "hit" }
    match tag {
        case found: Tag<int> => found.label
        case _ => "miss"
    }
}
main()
"#,
    );
    assert!(
        phantom.contains("(\"nominal_record\", \"Tag\","),
        "{phantom}"
    );
    assert!(phantom.contains("\"label\""), "{phantom}");
    assert_generated_python_gates(&phantom)
        .unwrap_or_else(|e| panic!("phantom generic nominal typed pattern failed: {e}"));
}

#[test]
fn bare_qualified_generic_nominal_declines_before_python_emission() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "lib.tpz",
        "export record Box<T> { value: T }\nexport let value: Box<int> = Box { value: 7 }\n",
    );
    provider.add_file(
        "main.tpz",
        "import lib\nmatch lib.value {\ncase found: lib.Box => 1\ncase _ => 0\n}\n",
    );
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_20);
    assert!(unit.diagnostics.is_empty(), "{:?}", unit.diagnostics);
    let error = emit_module(&unit).expect_err("missing type argument declines");
    assert_eq!(error.code(), "TPZ6PY0001");
    assert!(matches!(
        error.kind,
        PyEmitErrorKind::Unsupported("typed pattern type")
    ));
}

#[test]
fn bare_named_generic_nominals_decline_before_python_emission() {
    for (kind, source, files) in [
        (
            "local",
            "record Box<T> { value: T }\nlet value: Box<int> = Box { value: 7 }\nmatch value {\ncase found: Box => 1\ncase _ => 0\n}\n",
            Vec::new(),
        ),
        (
            "selected",
            "import lib { Box as ForeignBox, value }\nmatch value {\ncase found: ForeignBox => 1\ncase _ => 0\n}\n",
            vec![(
                "lib.tpz",
                "export enum Box<T> { Value(T) }\nexport let value: Box<int> = Box.Value(7)\n",
            )],
        ),
    ] {
        let mut provider = InMemoryProvider::new();
        provider.add_file("main.tpz", source);
        for (path, contents) in files {
            provider.add_file(path, contents);
        }
        let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_20);
        assert!(
            unit.diagnostics.is_empty(),
            "{kind}: {:?}",
            unit.diagnostics
        );
        let error = emit_module(&unit).expect_err(kind);
        assert_eq!(error.code(), "TPZ6PY0001", "{kind}");
        assert!(
            matches!(
                error.kind,
                PyEmitErrorKind::Unsupported("typed pattern type")
            ),
            "{kind}"
        );
    }
}

#[test]
fn generic_nominal_type_spec_cycles_decline_for_typed_let_and_pattern() {
    let typed_let = emit_error_for_source(
        r#"
record Tree<T> { child: Option<Tree<T>> }
function main() -> int {
    let t: Tree<int> = Tree { child: None }
    0
}
main()
"#,
    );
    assert_eq!(typed_let.code(), "TPZ6PY0001");
    assert!(matches!(
        typed_let.kind,
        PyEmitErrorKind::Unsupported("typed pattern type")
    ));

    let typed_pattern = emit_error_for_source(
        r#"
record Tree<T> { child: Option<Tree<T>> }
function make() -> Tree<int> {
    Tree { child: None }
}
function main() -> int {
    let t = make()
    match t {
        case found: Tree<int> => 1
        case _ => 0
    }
}
main()
"#,
    );
    assert_eq!(typed_pattern.code(), "TPZ6PY0001");
    assert!(matches!(
        typed_pattern.kind,
        PyEmitErrorKind::Unsupported("typed pattern type")
    ));

    let growing = emit_error_for_source(
        r#"
record Grow<T> { next: Option<Grow<Array<T>>> }
function make() -> Grow<int> {
    Grow { next: None }
}
function main() -> int {
    let g = make()
    match g {
        case found: Grow<int> => 1
        case _ => 0
    }
}
main()
"#,
    );
    assert_eq!(growing.code(), "TPZ6PY0001");
    assert!(matches!(
        growing.kind,
        PyEmitErrorKind::Unsupported("typed pattern type")
    ));
}

#[test]
fn generic_nominal_type_spec_declines_unsupported_function_field_type_without_panic() {
    let error = emit_error_for_source(
        r#"
record FnBox<T> { value: T }
function inc(x: int) -> int {
    x + 1
}
function main() -> int {
    let b = FnBox { value: inc }
    match b {
        case found: FnBox<(int) -> int> => 1
        case _ => 0
    }
}
main()
"#,
    );
    assert_eq!(error.code(), "TPZ6PY0001");
    assert!(matches!(
        error.kind,
        PyEmitErrorKind::Unsupported("typed pattern type")
    ));
}

#[test]
fn emits_guarded_match_arms_with_bindings() {
    let generated = emit_source(
        r#"
function main() -> int {
    let a = match 5 {
        case n if n > 5 => 100
        case n if n > 3 => n
        case _ => 0
    }
    match a {
        case n if n == 5 => print("ok")
        case _ => print("bad")
    }
    a
}
main()
"#,
    );
    assert!(generated.contains("tpz_condition"), "{generated}");
    assert!(generated.contains("lambda _t_6e"), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("guarded match Python gate failed: {e}"));
}

#[test]
fn direct_match_guards_bind_pattern_scope_and_callable_metadata() {
    let generated = emit_source(
        r#"
function add(a: int, b: int = 2, c: int = 4) -> int {
    a * 100 + b * 10 + c
}
function main() -> int {
    let n = 1
    let mut statementShadow = 0
    match 5 {
        case n if n > 3 => { statementShadow = n }
        case _ => ()
    }
    let valueShadow = match 6 {
        case n if n > 3 => loop { break n }
        case _ => 0
    }

    let callbacks = map { "plus": add }
    let mut statementCallable = 0
    match callbacks.get("plus") {
        case Some(cb) if cb(c: 5, a: 1) == 125 => { statementCallable = 1 }
        case _ => ()
    }
    let valueCallable = match callbacks.get("plus") {
        case Some(cb) if cb(c: 5, a: 1) == 125 => loop { break 2 }
        case _ => 0
    }

    statementShadow * 1000 + valueShadow * 100 + statementCallable * 10 + valueCallable
}
main()
"#,
    );
    assert_generated_python_ok_int(
        &generated,
        5_612,
        "direct match guard pattern scope and callable metadata",
    );
}

#[test]
fn emits_returning_match_arms_through_tpz_return() {
    let generated = emit_source(
        r#"
function f(n: int) -> int {
    let x = match n {
        case 0 => return 7
        case _ => n
    }
    x + 1
}
function g(n: int) -> int {
    match n {
        case 0 => return 5
        case _ => print("go")
    }
    6
}
function main() -> int {
    f(0) + f(3) + g(0) + g(1)
}
main()
"#,
    );
    assert!(generated.contains("tpz_return(7)"), "{generated}");
    assert!(generated.contains("raise TpzReturn(5)"), "{generated}");
    assert_generated_python_gates(&generated)
        .unwrap_or_else(|e| panic!("returning match arm Python gate failed: {e}"));
}

#[test]
fn declines_unchecked_top_level_match_return_arms_before_python_emit() {
    let cases = [
        (
            "entry_final_match_return_arm",
            r#"
match true {
    case true => return 1
    case _ => 0
}
"#,
            &[][..],
        ),
        (
            "entry_let_match_return_arm",
            r#"
let x = match true {
    case true => return 1
    case _ => 0
}
print("{x}")
"#,
            &[][..],
        ),
        (
            "entry_concurrent_else_return",
            r#"
let x = concurrent(timeout: 0ms) {
    a: 1
    b: 2
} else {
    return 1
}
x.a
"#,
            &[][..],
        ),
        (
            "entry_loop_return",
            r#"
loop {
    return 1
}
"#,
            &[][..],
        ),
        (
            "entry_loop_break_try",
            r#"
function fail() -> Result<int, string> {
    return Err("x")
}
loop {
    break fail()?
}
"#,
            &[][..],
        ),
        (
            "imported_let_match_return_arm",
            r#"
import util { answer }
answer
"#,
            &[(
                "util.tpz",
                r#"
export let answer = match true {
    case true => return 1
    case _ => 0
}
"#,
            )][..],
        ),
    ];

    for (name, src, files) in cases {
        let error = emit_unchecked_error_for_source_with_files(src, files);
        assert_eq!(error.code(), "TPZ6PY0001", "{name}");
        assert_eq!(
            error.kind,
            PyEmitErrorKind::Unsupported("return outside a function"),
            "{name}"
        );
        assert!(
            error.span.is_some(),
            "{name}: unsupported return decline must carry a span"
        );
    }
}
