use super::*;

#[test]
fn measured_python_constructs_are_compiled_or_checker_owned() {
    let cases = [
        UnsupportedCase {
            name: "concurrent_timeout_expression",
            what: "concurrent timeout",
            src: r#"
function f() -> int { 1 }
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        a: f()
    } else {
        { a: 0 }
    }
    r.a
}
"#,
        },
        UnsupportedCase {
            name: "concurrent_zero_timeout_multi_noninstant",
            what: "concurrent timeout",
            src: r#"
function f() -> int { 1 }
function main() -> int {
    let r = concurrent(timeout: 0ms) {
        x: f()
        y: 2
    } else {
        { x: 0, y: 0 }
    }
    r.x
}
main()
"#,
        },
        UnsupportedCase {
            name: "assign_to_immutable",
            what: "assign to immutable",
            src: r#"
function main() -> int {
    let x = 1
    x = 2
    x
}
main()
"#,
        },
        UnsupportedCase {
            name: "nested_function_forward_reference",
            what: "nested function forward reference",
            src: r#"
function main() -> int {
    log()
    function log() -> int {
        1
    }
    0
}
main()
"#,
        },
        UnsupportedCase {
            name: "nested_function_transitive_forward_reference",
            what: "nested function forward reference",
            src: r#"
function main() -> int {
    function first() -> int {
        second()
    }
    first()
    function second() -> int {
        1
    }
    0
}
main()
"#,
        },
        UnsupportedCase {
            name: "nested_function_multistep_transitive_forward_reference",
            what: "nested function forward reference",
            src: r#"
function main() -> int {
    function caller() -> int {
        first()
    }
    function first() -> int {
        second()
    }
    caller()
    function second() -> int {
        1
    }
    0
}
main()
"#,
        },
        UnsupportedCase {
            name: "nested_function_branch_local_transitive_forward_reference",
            what: "nested function forward reference",
            src: r#"
function main() -> int {
    function first() -> int {
        second()
    }
    if true {
        first()
    }
    function second() -> int {
        1
    }
    0
}
main()
"#,
        },
        UnsupportedCase {
            name: "nested_function_dead_branch_transitive_forward_reference",
            what: "nested function forward reference",
            src: r#"
function main() -> int {
    function first() -> int {
        if false {
            second()
        } else {
            3
        }
    }
    first()
    function second() -> int {
        1
    }
    0
}
main()
"#,
        },
        UnsupportedCase {
            name: "nested_function_iife_forward_reference",
            what: "nested function forward reference",
            src: r#"
function main() -> int {
    let value = (() => later())()
    function later() -> int {
        1
    }
    value
}
main()
"#,
        },
        UnsupportedCase {
            name: "nested_function_parenthesized_iife_forward_reference",
            what: "nested function forward reference",
            src: r#"
function main() -> int {
    let value = ((() => later()))()
    function later() -> int {
        1
    }
    value
}
main()
"#,
        },
        UnsupportedCase {
            name: "nested_function_iife_transitive_forward_reference",
            what: "nested function forward reference",
            src: r#"
function main() -> int {
    function first() -> int {
        second()
    }
    let value = (() => first())()
    function second() -> int {
        1
    }
    value
}
main()
"#,
        },
        UnsupportedCase {
            name: "nested_function_iife_shadowed_outer_function",
            what: "nested function forward reference",
            src: r#"
function later() -> int {
    0
}
function main() -> int {
    let value = (() => later())()
    function later() -> int {
        1
    }
    value
}
main()
"#,
        },
        UnsupportedCase {
            name: "nested_function_parameter_default_forward",
            what: "function default shape",
            src: r#"
function main() -> int {
    function first(x: int = second()) -> int {
        x
    }
    function second() -> int {
        1
    }
    first()
}
main()
"#,
        },
    ];

    assert_eq!(cases.len(), 13, "update the Stage 8 diagnostic matrix");
    for case in cases {
        if !matches!(
            case.name,
            "assign_to_immutable" | "nested_function_parameter_default_forward"
        ) {
            let generated = emit_source(case.src);
            assert_generated_python_gates(&generated).unwrap_or_else(|error| {
                panic!(
                    "{}: accepted construct Python gate failed: {error}",
                    case.name
                )
            });
            continue;
        }
        let error = emit_error_for_source(case.src);
        assert_eq!(error.code(), "TPZ6PY0001", "{}", case.name);
        match error.kind {
            PyEmitErrorKind::Unsupported(what) => {
                assert_eq!(what, case.what, "{}", case.name);
            }
            other => panic!("{}: expected unsupported error, got {other:?}", case.name),
        }
        let span = error
            .span
            .unwrap_or_else(|| panic!("{}: unsupported error must carry a span", case.name));
        assert!(
            span.hi > span.lo && (span.hi as usize) <= case.src.len(),
            "{}: invalid span {:?}",
            case.name,
            span
        );
        let span_text = &case.src[span.lo as usize..span.hi as usize];
        assert!(
            !span_text.trim().is_empty(),
            "{}: span points at empty text {:?}",
            case.name,
            span
        );
    }
}

#[test]
fn af008_erased_type_arguments_and_typed_json_calls_emit() {
    struct Case {
        name: &'static str,
        src: &'static str,
    }
    let erased_cases = [
        Case {
            name: "erased-direct",
            src: "function id<T>(x: T) -> T { x }\nid<int>(1)",
        },
        Case {
            name: "erased-statement-lowered",
            src: "function id<T>(x: T) -> T { x }\nlet x = if true { id<int>(1) } else { 0 }\nx",
        },
        Case {
            name: "erased-pipe",
            src: "function id<T>(x: T) -> T { x }\n1 |> id<int>()",
        },
    ];
    for case in erased_cases {
        let emitted = emit_source(case.src);
        assert!(
            !emitted.is_empty(),
            "{}: erased explicit type arguments must emit",
            case.name
        );
    }

    let typed_json_cases = [
        Case {
            name: "typed-json-direct",
            src: "match JSON.parseAs<int>(\"1\") {\ncase Ok(n) => n\ncase Err(_) => 0\n}",
        },
        Case {
            name: "typed-json-statement-lowered",
            src: "let x = if true { JSON.parseAs<int>(\"1\") } else { Err(\"x\") }\nmatch x {\ncase Ok(n) => n\ncase Err(_) => 0\n}",
        },
        Case {
            name: "typed-json-pipe",
            src: "match (\"1\" |> JSON.parseAs<int>()) {\ncase Ok(n) => n\ncase Err(_) => 0\n}",
        },
        Case {
            name: "typed-json-named",
            src: "match JSON.parseAs<int>(text: \"1\") {\ncase Ok(n) => n\ncase Err(_) => 0\n}",
        },
    ];

    for case in typed_json_cases {
        let generated = emit_source(case.src);
        assert!(
            generated.contains("tpz_json_parse_as("),
            "{}: typed JSON schema must reach the runtime leaf: {generated}",
            case.name
        );
        assert_generated_python_ok_int(&generated, 1, case.name);
    }
}

#[test]
fn af008_typed_json_decodes_local_generic_nominals_aliases_and_json_values() {
    let generated = emit_source(
        r#"
type Scalar<T> = T
type Lookup = Map<string, int>
record Box<T> { value: Scalar<T>, lookup: Lookup, fallback: int = 3 }
enum Cell<T> { Value(T) }
newtype Wrapped<T> = T
function main() -> int {
    let boxed = match JSON.parseAs<Box<int>>("\{\"value\":7,\"lookup\":\{\"answer\":8\}\}") {
        case Ok(value) => value
        case Err(_) => Box { value: 0, lookup: Map.new(), fallback: 0 }
    }
    let cell = match JSON.parseAs<Cell<int>>("\{\"tag\":\"Value\",\"values\":[9]\}") {
        case Ok(Value(value)) => value
        case _ => 0
    }
    let wrapped = match JSON.parseAs<Wrapped<Array<int>>>("[4,5]") {
        case Ok(Wrapped(values)) => values[1]
        case _ => 0
    }
    let decoded = match JSON.parse("\{\"n\":6\}") {
        case Ok(json) => match JSON.decode<{ n: int }>(json) {
            case Ok(value) => value.n
            case Err(_) => 0
        }
        case Err(_) => 0
    }
    boxed.value + boxed.lookup.getOr("answer", 0) + boxed.fallback + cell + wrapped + decoded
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_json_decode(")
            && generated.contains("(\"record\", \"Box\"")
            && generated.contains("(\"enum\", \"Cell\"")
            && generated.contains("(\"newtype\", \"Wrapped\""),
        "typed JSON must materialize local schemas: {generated}"
    );
    assert_generated_python_ok_int(&generated, 38, "typed nominal JSON");
}

#[test]
fn v520_typed_json_resolves_selected_qualified_and_nested_imported_schemas() {
    let generated = emit_source_with_files_and_version(
        r#"
import model
import selected { Box, UserAlias }
let qualified = match JSON.parseAs<model.User>("\{\"name\":\"Ada\"\}") {
    case Ok(user) => user.rank + 1
    case Err(_) => 0
}
let aliased = match JSON.parseAs<UserAlias>("\{\"name\":\"Bea\"\}") {
    case Ok(user) => if user.name == "Bea" { 1 } else { 0 }
    case Err(_) => 0
}
let generic = match JSON.parseAs<Box<int>>("\{\"value\":7,\"rank\":8\}") {
    case Ok(boxed) => boxed.value + boxed.rank
    case Err(_) => 0
}
qualified + aliased + generic
"#,
        &[
            (
                "scalar.tpz",
                "export type Scalar = int\nexport record Hidden { name: string }\n",
            ),
            (
                "model.tpz",
                "import scalar { Scalar }\nexport record User { name: string, rank: Scalar = 0 }\n",
            ),
            (
                "selected.tpz",
                "import scalar { Scalar, Hidden }\nexport type UserAlias = Hidden\nexport record Box<T> { value: T, rank: Scalar }\n",
            ),
        ],
        LangVersion::V5_20,
    );
    assert!(
        generated.contains("model::User")
            && generated.contains("scalar::Hidden")
            && generated.contains("selected::Box"),
        "5.20 imported schemas must retain their defining modules: {generated}"
    );
    assert_generated_python_ok_int(&generated, 17, "5.20 imported typed JSON");
}

#[test]
fn v520_same_spelled_nominals_keep_value_key_and_pattern_identity() {
    let generated = emit_source_with_files_and_version(
        r#"
import alpha { User as AlphaUser, Code as AlphaCode, Flag as AlphaFlag }
import beta { User as BetaUser, Code as BetaCode, Flag as BetaFlag }
let alpha = AlphaUser { id: 1 }
let beta = BetaUser { id: 1 }
let alphaCode = AlphaCode(1)
let betaCode = BetaCode(1)
let alphaFlag = AlphaFlag.On
let betaFlag = BetaFlag.On
let users = Set.of(alpha, beta)
let mut labels = Map.new()
labels.insert(alpha, "alpha")
labels.insert(beta, "beta")
let pattern = match alpha {
    case BetaUser { id } => 9
    case AlphaUser { id } => id
}
let codePattern = match alphaCode {
    case BetaCode(value) => 9
    case AlphaCode(value) => value
}
let flagPattern = match alphaFlag {
    case value: BetaFlag => 9
    case value: AlphaFlag => 1
}
let equality = (if alpha == beta { 100 } else { 0 })
    + (if alphaCode == betaCode { 1000 } else { 0 })
    + (if alphaFlag == betaFlag { 10000 } else { 0 })
let result = equality + users.length * 10 + labels.length + pattern + codePattern + flagPattern
result
"#,
        &[
            (
                "alpha.tpz",
                "export record User { id: int }\nexport newtype Code = int\nexport enum Flag { On }\n",
            ),
            (
                "beta.tpz",
                "export record User { id: int }\nexport newtype Code = int\nexport enum Flag { On }\n",
            ),
        ],
        LangVersion::V5_20,
    );
    assert!(
        generated.contains("alpha::User") && generated.contains("beta::User"),
        "5.20 nominal values must carry declaration identities: {generated}"
    );
    assert_generated_python_ok_int(&generated, 25, "5.20 nominal identity");
}

#[test]
fn af008_typed_json_errors_are_values_and_wrong_kinds_fault() {
    let mismatch = emit_source(
        r#"
match JSON.parseAs<Array<int>>("[1,\"x\"]") {
    case Ok(_) => "unexpected"
    case Err(error) => error
}
"#,
    );
    assert_generated_python_ok_string(
        &mismatch,
        "$[1]: expected int, found string",
        "typed JSON mismatch path",
    );

    let parse_error = emit_source(
        r#"
match JSON.parseAs<int>("\{") {
    case Ok(_) => "unexpected"
    case Err(error) => error
}
"#,
    );
    assert_generated_python_ok_string(
        &parse_error,
        "$: invalid JSON at line 1, column 2: expected a string key in object",
        "typed JSON parse error",
    );

    let wrong_kind = emit_unchecked_source("JSON.decode<int>(1)");
    assert_generated_python_fault_code(&wrong_kind, "TPZ5001", "unchecked JSON.decode wrong kind");

    let statement_lowered_pipe_spread = emit_source(
        r#"
let result = if true {
    "1" |> JSON.parseAs<int>(...[])
} else {
    Ok(0)
}
result
"#,
    );
    assert_generated_python_fault_code(
        &statement_lowered_pipe_spread,
        "TPZ5004",
        "statement-lowered pipe spread fault",
    );
}

#[test]
fn af009_statement_lowered_nominal_spread_preserves_order_and_identity() {
    let src = r#"
record User { name: string, age: int, score: int }
function main() -> int {
    let mut order = 0
    let base: User = User { name: "Ada", age: 1, score: 5 }
    let next: User = User {
        ...if true {
            order = order * 10 + 1
            base
        } else { base },
        age: loop {
            order = order * 10 + 2
            break 7
        },
        score: (for x in [3] {
            order = order * 10 + x
            x * 10
        })[0],
    }
    let direct: User = User {
        name: "Grace",
        age: loop {
            order = order * 10 + 4
            break 8
        },
        score: 9,
    }
    order * 100000 + next.age * 10000 + next.score * 100 + direct.age * 10 + base.age
}
main()
"#;
    let generated = emit_source(src);
    assert!(
        generated.contains("def __tpz_nominal_spread_")
            && generated.matches("def __tpz_nominal_field_").count() >= 3
            && generated.contains("tpz_nominal_record("),
        "operands must lower through ordered nominal thunks: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        123_473_081,
        "statement-lowered nominal spread order",
    );
}

#[test]
fn af009_wrong_nominal_spread_skips_statement_lowered_replacement() {
    let generated = emit_source(
        r#"
record User { value: int }
record Other { value: int }
function main() -> User {
    let other: Other = Other { value: 1 }
    User {
        ...if true { other } else { other },
        value: loop { break 1 / 0 },
    }
}
main()
"#,
    );
    assert_generated_python_fault_code(
        &generated,
        "TPZ5001",
        "wrong nominal spread skips replacement",
    );
}

#[test]
fn af009_statement_lowered_nominal_spread_is_cooperative_in_concurrent_arms() {
    let generated = emit_source(
        r#"
record User { value: int }
function main() -> int {
    let result = concurrent {
        update: {
            let base: User = User { value: 1 }
            let next: User = User {
                ...if true { base } else { base },
                value: loop {
                    let mut i = 0
                    while i < 2 { i = i + 1 }
                    break 9
                },
            }
            next.value
        }
        idle: 0
    }
    result.update
}
main()
"#,
    );
    assert!(
        generated.contains("yield from tpz_nominal_record__co(")
            && generated.contains("def __tpz_nominal_spread_")
            && generated.contains("def __tpz_nominal_field_"),
        "cooperative operands must stay on the scheduler: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        9,
        "cooperative statement-lowered nominal spread",
    );
}

#[test]
fn af009_selected_import_statement_lowered_nominal_spread_preserves_identity() {
    let generated = emit_source_with_files(
        r#"
import model { User, seed }
function main() -> int {
    let next: User = User {
        ...if true { seed } else { seed },
        value: loop { break 8 },
    }
    next.value
}
main()
"#,
        &[(
            "model.tpz",
            r#"
export record User { value: int }
export let seed: User = User { value: 1 }
"#,
        )],
    );
    assert!(
        generated.contains("tpz_nominal_record(_tnr_t_6d6f64656c___t_55736572")
            && generated.contains("def __tpz_nominal_spread_")
            && generated.contains("def __tpz_nominal_field_"),
        "selected-import lowering must retain the defining nominal class: {generated}"
    );
    assert_generated_python_ok_int(
        &generated,
        8,
        "selected-import statement-lowered nominal spread",
    );
}

#[test]
fn using_statement_preserves_single_acquisition_defer_and_closed_capture_order() {
    let generated = emit_source(
        r#"
function main() -> int {
    let mut acquisitions = 0
    let mut later = match open("config.txt") {
        case Ok(file) => file
        case Err(_) => loop {}
    }
    function acquire() -> Result<File, string> {
        acquisitions = acquisitions + 1
        open("config.txt")
    }
    function scenario() -> Result<int, string> {
        using file = acquire()? {
            later = file
            print("body")
            defer print("defer")
            999
        }
        Ok(7)
    }
    function explicit() -> Result<int, string> {
        using file = acquire()? {
            let ignored = file.close()
            print("explicit")
        }
        Ok(3)
    }
    let value = match scenario() {
        case Ok(n) => n
        case Err(_) => 0
    }
    let explicitValue = match explicit() {
        case Ok(n) => n
        case Err(_) => 0
    }
    let observed = match later.read() {
        case Ok(text) => text
        case Err(_) => "closed"
    }
    print(observed)
    acquisitions * 10 + value + explicitValue
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_using_file(")
            && generated.contains("except (TpzReturn, TpzLoopBreak, TpzLoopContinue):")
            && generated.matches("tpz_file_close(").count() >= 2,
        "using must guard and close on normal/crossing paths: {generated}"
    );
    assert_generated_python_ok_int_with_files_and_stdout(
        &generated,
        30,
        &[("config.txt", "hello")],
        &["body", "defer", "explicit", "closed"],
        "using normal/defer/capture order",
    );
}

#[test]
fn using_statement_closes_across_return_question_break_and_continue() {
    let generated = emit_source(
        r#"
function getFile() -> File {
    match open("config.txt") {
        case Ok(file) => file
        case Err(_) => loop {}
    }
}
function early(file: File) -> int {
    using held = file { return 8 }
    0
}
function question(file: File) -> Result<int, string> {
    using held = file {
        let value = Err("stop")?
        Ok(value)
    }
    Ok(0)
}
function main() -> int {
    let returned = early(getFile())
    let failed = match question(getFile()) {
        case Err(text) if text == "stop" => 1
        case _ => 0
    }
    let mut iterations = 0
    loop 'outer {
        iterations = iterations + 1
        using held = getFile() {
            if iterations == 1 { continue 'outer }
            break 'outer
        }
    }
    returned * 100 + failed * 10 + iterations
}
main()
"#,
    );
    assert_generated_python_ok_int_with_files_and_stdout(
        &generated,
        812,
        &[("config.txt", "hello")],
        &[],
        "using crossing control",
    );
}

#[test]
fn using_statement_guards_non_file_before_body_execution() {
    let generated = emit_source(
        r#"
function main() -> int {
    using value = 1 {
        print("body")
    }
    0
}
main()
"#,
    );
    assert!(
        generated.find("tpz_using_file(") < generated.find("print("),
        "using guard must precede body emission: {generated}"
    );
    assert_generated_python_fault_code(&generated, "TPZ5001", "using non-File guard");
}

#[test]
fn using_statement_preserves_cooperative_body_checkpoints() {
    let generated = emit_source(
        r#"
function getFile() -> File {
    match open("config.txt") {
        case Ok(file) => file
        case Err(_) => loop {}
    }
}
function main() -> int {
    let result = concurrent {
        use: {
            using file = getFile() {
                let mut i = 0
                while i < 3 { i = i + 1 }
                99
            }
            7
        }
        idle: 0
    }
    result.use
}
main()
"#,
    );
    assert!(
        generated.contains("yield None")
            && generated.contains("tpz_using_file(")
            && generated.contains("tpz_file_close("),
        "using inside a cooperative arm must retain scheduler checkpoints: {generated}"
    );
    assert_generated_python_ok_int_with_files_and_stdout(
        &generated,
        7,
        &[("config.txt", "hello")],
        &[],
        "cooperative using body",
    );
}

#[test]
fn unit_typed_match_pattern_uses_runtime_unit_spec() {
    let generated = emit_source(
        r#"
function main() -> int {
    match () {
        case n: () => 1
        case _ => 0
    }
}
main()
"#,
    );
    assert!(
        generated.contains("tpz_type_matches(__tpz_match, \"unit\")"),
        "unit typed pattern must use the shared runtime type spec: {generated}"
    );
    assert_generated_python_ok_int(&generated, 1, "unit typed match pattern");
}

#[test]
fn unchecked_wrong_named_array_get_receiver_call_still_declines_at_emitter_boundary() {
    let src = r#"
function main() -> Option<int> {
    let xs = [1]
    xs.get(k: 0)
}
"#;

    let error = emit_error_for_source(src);
    assert_eq!(error.code(), "TPZ6PY0001");
    match error.kind {
        PyEmitErrorKind::Unsupported(what) => {
            assert_eq!(what, "call argument shape");
        }
        other => panic!("expected unsupported error, got {other:?}"),
    }
    let span = error
        .span
        .unwrap_or_else(|| panic!("wrong named receiver call decline must carry a span"));
    let span_text = &src[span.lo as usize..span.hi as usize];
    assert!(
        span_text.contains("k"),
        "span must point at the wrong named argument: {span_text:?}"
    );
}

#[test]
fn unchecked_concrete_non_callable_local_calls_still_decline_at_emitter_boundary() {
    let cases = [
        (
            "zero_arg",
            r#"
function main() -> int {
    let f = 1
    f()
}
"#,
        ),
        (
            "positional",
            r#"
function main() -> int {
    let f = 1
    f(1)
}
"#,
        ),
    ];

    for (name, src) in cases {
        let error = emit_error_for_source(src);
        assert_eq!(error.code(), "TPZ6PY0001", "{name}");
        match error.kind {
            PyEmitErrorKind::Unsupported(what) => {
                assert_eq!(what, "call target", "{name}");
            }
            other => panic!("{name}: expected unsupported error, got {other:?}"),
        }
        let span = error
            .span
            .unwrap_or_else(|| panic!("{name}: call-target decline must carry a span"));
        let span_text = &src[span.lo as usize..span.hi as usize];
        assert_eq!(span_text, "f", "{name}: span must point at the callee");
    }
}

#[test]
fn type_alias_declarations_erase_from_generated_python() {
    let cases = [
        UnsupportedCaseWithFiles {
            name: "exported_type_alias",
            src: r#"
export type UserId = int
0
"#,
            files: &[],
        },
        UnsupportedCaseWithFiles {
            name: "type_alias_statement",
            src: r#"
type UserId = int
0
"#,
            files: &[],
        },
    ];

    assert_eq!(cases.len(), 2, "update the Stage 8 diagnostic matrix");
    for case in cases {
        let generated = emit_source_with_files(case.src, case.files);
        assert_generated_python_gates(&generated)
            .unwrap_or_else(|error| panic!("{}: alias erasure gate failed: {error}", case.name));
    }
}

#[test]
fn receiver_impl_emits_registered_direct_call() {
    let src = r#"
record User { name: string }
impl User {
    function label(self) -> string { self.name }
}
let user: User = User { name: "Ada" }
if user.label() == "Ada" { 1 } else { 0 }
"#;
    let generated = emit_source(src);
    assert!(
        generated.contains("__tpz_methods[(")
            && generated.contains("tpz_bound_user_method(__tpz_methods")
            && generated.contains("__topaz_method_identity__ = \"__entry__::User\""),
        "receiver method registry must be emitted: {generated}"
    );
    assert_generated_python_ok_int(&generated, 1, "local receiver impl");
}

#[test]
fn exported_receiver_impl_runs_across_a_namespace_import() {
    let generated = emit_source_with_files(
        "import model\nlet p: model.Point = model.make(4)\nif p.coordinate() == 4 { 1 } else { 0 }",
        &[(
            "model.tpz",
            "export record Point { x: int }\nimpl Point { export function coordinate(self) -> int { self.x } }\nexport function make(x: int) -> Point { Point { x: x } }",
        )],
    );
    assert!(
        generated.contains("model::Point")
            && generated.contains("tpz_bound_user_method(__tpz_methods"),
        "cross-module receiver identity must be preserved: {generated}"
    );
    assert_generated_python_ok_int(&generated, 1, "exported receiver impl");
}

#[test]
fn exported_receiver_method_reads_its_qualified_module_value() {
    let generated = emit_source_with_files(
        "import model\nlet p: model.Point = model.make()\np.shifted() * 10 + model.offset",
        &[(
            "model.tpz",
            "export record Point { x: int }\nexport let offset = 2\nimpl Point { export function shifted(self) -> int { self.x + offset } }\nexport function make() -> Point { Point { x: 1 } }",
        )],
    );
    assert_generated_python_ok_int(&generated, 32, "exported method module value");
}

#[test]
fn same_spelled_exported_receiver_impls_remain_disjoint() {
    let generated = emit_source_with_files(
        "import left\nimport right\nlet a: left.Point = left.make()\nlet b: right.Point = right.make()\na.coordinate() * 10 + b.coordinate()",
        &[
            (
                "left.tpz",
                "export record Point { x: int }\nimpl Point { export function coordinate(self) -> int { self.x } }\nexport function make() -> Point { Point { x: 1 } }",
            ),
            (
                "right.tpz",
                "export record Point { x: int }\nimpl Point { export function coordinate(self) -> int { self.x } }\nexport function make() -> Point { Point { x: 2 } }",
            ),
        ],
    );
    assert!(generated.contains("left::Point") && generated.contains("right::Point"));
    assert_generated_python_ok_int(&generated, 12, "disjoint receiver identities");
}

#[test]
fn same_named_private_module_values_remain_disjoint_in_methods() {
    let generated = emit_source_with_files(
        "import left\nimport right\nlet a: left.Point = left.make()\nlet b: right.Point = right.make()\na.coordinate() * 10 + b.coordinate()",
        &[
            (
                "left.tpz",
                "export record Point { x: int }\nlet offset = 1\nimpl Point { export function coordinate(self) -> int { self.x + offset } }\nexport function make() -> Point { Point { x: 1 } }",
            ),
            (
                "right.tpz",
                "export record Point { x: int }\nlet offset = 2\nimpl Point { export function coordinate(self) -> int { self.x + offset } }\nexport function make() -> Point { Point { x: 2 } }",
            ),
        ],
    );
    assert_generated_python_ok_int(&generated, 24, "private module method captures");
}

#[test]
fn qualified_private_module_values_support_forward_function_reads() {
    let generated = emit_source_with_files(
        "import model\nmodel.read()",
        &[(
            "model.tpz",
            "export function read() -> int { offset }\nlet offset = 2",
        )],
    );
    assert_generated_python_ok_int(&generated, 2, "forward private module read");
}

#[test]
fn same_named_receiver_methods_with_different_signatures_bind_dynamically() {
    let generated = emit_source(
        r#"
record Fixed { base: int }
record Bag { base: int }
impl Fixed { function total(self, value: int) -> int { self.base + value } }
impl Bag {
    function total(self, ...values: int) -> int {
        let mut result = self.base
        for value in values { result += value }
        result
    }
}
let fixed: Fixed = Fixed { base: 10 }
let bag: Bag = Bag { base: 20 }
fixed.total(3) * 100 + bag.total(...[1, 2])
"#,
    );
    assert!(generated.contains("tpz_user_method_call("), "{generated}");
    assert_generated_python_ok_int(&generated, 1323, "dynamic receiver signatures");
}

#[test]
fn ambiguous_receiver_signature_uses_dynamic_cooperative_binding() {
    let generated = emit_source(
        r#"
record Fixed { base: int }
record Bag { base: int }
impl Fixed { function total(self, value: int) -> int { self.base + value } }
impl Bag {
    function total(self, ...values: int) -> int {
        let mut result = self.base
        for value in values { result += value }
        result
    }
}
function main() -> int {
    let bag: Bag = Bag { base: 20 }
    let results = concurrent {
        work: bag.total(...[1, 2])
        idle: 0
    }
    results.work
}
main()
"#,
    );
    assert!(
        generated.contains("yield from tpz_user_method_call_cooperative("),
        "{generated}"
    );
    assert_generated_python_ok_int(&generated, 23, "dynamic cooperative receiver signature");
}

#[test]
fn receiver_method_module_value_preinit_is_a_topaz_unbound_fault() {
    let generated = emit_source(
        r#"
record Point { x: int }
impl Point { function shifted(self) -> int { self.x + offset } }
Point { x: 1 }.shifted()
let offset = 2
0
"#,
    );
    assert!(
        generated.contains("__tpz_module_value(")
            && generated.contains("globals()[\"_t_6f6666736574\"] = __tpz_missing"),
        "{generated}"
    );
    assert_generated_python_fault_code(&generated, "TPZ5002", "method capture preinit");
}

#[test]
fn receiver_impl_reuses_named_default_and_variadic_call_abi() {
    let generated = emit_source(
        r#"
record Box { base: int }
impl Box {
    function scaled(self, scale: int = 2) -> int { self.base * scale }
    function count(self, ...rest: int) -> int { self.base + rest.length }
}
let box: Box = Box { base: 5 }
box.scaled() + box.scaled(scale: 3) + box.count(7, 8)
"#,
    );
    assert_generated_python_ok_int(&generated, 32, "receiver method call ABI");
}

#[test]
fn receiver_impl_call_uses_cooperative_sibling_in_concurrent_arms() {
    let generated = emit_source(
        r#"
record Counter { base: int }
impl Counter {
    function spin(self) -> int {
        let mut i = 0
        while i < 3 { i = i + 1 }
        self.base + i
    }
}
function main() -> int {
    let result = concurrent {
        work: Counter { base: 5 }.spin()
        idle: 0
    }
    result.work
}
main()
"#,
    );
    assert!(
        generated.contains("yield from tpz_call_cooperative("),
        "cooperative receiver calls must dispatch to the cooperative method sibling: {generated}"
    );
    assert_generated_python_ok_int(&generated, 8, "cooperative receiver impl");
}

#[test]
fn derived_builtin_protocol_dispatch_runs_through_the_shared_leaf() {
    let generated = emit_source(
        r#"
record User derives Show { name: string }
let user: User = User { name: "Ada" }
Show.show(user)
"#,
    );
    assert!(generated.contains("tpz_protocol_call("), "{generated}");
    assert_generated_python_ok_string(
        &generated,
        "User { name: Ada }",
        "derived builtin protocol dispatch",
    );
}

#[test]
fn derived_order_protocol_dispatches_nominal_records() {
    let generated = emit_source(
        r#"
record Pair derives Order { a: int, b: int }
let left = Pair { a: 1, b: 2 }
let right = Pair { a: 1, b: 9 }
Order.compare(left, right)
"#,
    );
    assert_generated_python_ok_int(&generated, -1, "derived nominal order");
}

#[test]
fn manual_builtin_protocol_impl_dispatches_to_the_user_body() {
    let generated = emit_source(
        r#"
record User { name: string }
impl Show<User> {
    function show(value: User) -> string { value.name }
}
Show.show(User { name: "Ada" })
"#,
    );
    assert!(
        generated.contains("__tpz_protocol_") && generated.contains("__entry__::Show<User>"),
        "{generated}"
    );
    assert_generated_python_ok_string(&generated, "Ada", "manual builtin protocol impl");
}

#[test]
fn user_protocol_declaration_and_manual_impl_dispatch() {
    let generated = emit_source(
        r#"
protocol Label { function label(value: Self) -> string }
record User { name: string }
impl Label<User> {
    function label(value: User) -> string { value.name }
}
Label.label(User { name: "Ada" })
"#,
    );
    assert!(
        generated.contains("__entry__::Label<User>") && generated.contains("tpz_protocol_call("),
        "{generated}"
    );
    assert_generated_python_ok_string(&generated, "Ada", "user protocol dispatch");
}

#[test]
fn marker_protocol_declaration_erases_cleanly() {
    let generated = emit_source("protocol Marker {}\n0");
    assert_generated_python_ok_int(&generated, 0, "marker protocol");
}

#[test]
fn protocol_impl_reads_its_defining_module_value() {
    let generated = emit_source(
        r#"
protocol Shift { function shifted(value: Self) -> int }
record Point { x: int }
let offset = 2
impl Shift<Point> {
    function shifted(value: Point) -> int { value.x + offset }
}
Shift.shifted(Point { x: 1 })
"#,
    );
    assert!(
        generated.contains("__tpz_module_value(") && generated.contains("__entry__::Shift<Point>"),
        "{generated}"
    );
    assert_generated_python_ok_int(&generated, 3, "protocol module value capture");
}

#[test]
fn protocol_impl_call_uses_cooperative_sibling_in_concurrent_arms() {
    let generated = emit_source(
        r#"
protocol Spin { function spin(value: Self) -> int }
record Counter { base: int }
impl Spin<Counter> {
    function spin(value: Counter) -> int {
        let mut i = 0
        while i < 3 { i = i + 1 }
        value.base + i
    }
}
function main() -> int {
    let result = concurrent {
        work: Spin.spin(Counter { base: 5 })
        idle: 0
    }
    result.work
}
main()
"#,
    );
    assert!(
        generated.contains("yield from tpz_protocol_call_cooperative("),
        "{generated}"
    );
    assert_generated_python_ok_int(&generated, 8, "cooperative protocol impl");
}
