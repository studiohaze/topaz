//! §17 unit execution: resolve real multi-file units in memory and
//! run them (CDR-003 §9).

use topaz_interp::{Machine, TestHost, render};
use topaz_resolve::{InMemoryProvider, resolve, resolve_with_version};
use topaz_syntax::LangVersion;

fn run_unit(files: &[(&str, &str)], entry: &str) -> Result<(String, Vec<String>), String> {
    let mut provider = InMemoryProvider::new();
    for (path, contents) in files {
        provider.add_file(*path, *contents);
    }
    let unit = resolve(&provider, entry, None);
    assert!(
        unit.diagnostics.is_empty(),
        "unit must resolve: {:?}",
        unit.diagnostics
    );
    let host = TestHost::new();
    match Machine::run_unit(&unit, &host) {
        Ok(v) => Ok((render(&v), host.stdout())),
        Err(e) => Err(format!("{}: {}", e.code, e.message)),
    }
}

fn run_unit_with_version(
    files: &[(&str, &str)],
    entry: &str,
    version: LangVersion,
) -> Result<(String, Vec<String>), String> {
    let mut provider = InMemoryProvider::new();
    for (path, contents) in files {
        provider.add_file(*path, *contents);
    }
    let unit = resolve_with_version(&provider, entry, None, version);
    assert!(
        unit.diagnostics.is_empty(),
        "unit must resolve: {:?}",
        unit.diagnostics
    );
    let host = TestHost::new();
    match Machine::run_unit(&unit, &host) {
        Ok(value) => Ok((render(&value), host.stdout())),
        Err(error) => Err(format!("{}: {}", error.code, error.message)),
    }
}

#[test]
fn two_file_unit_runs() {
    let (_, stdout) = run_unit(
        &[
            (
                "utils/strings.tpz",
                "export function shout(s: string) -> string {\n    return \"{s}!\"\n}\n\nexport let greeting = \"hello\"\n",
            ),
            (
                "main.tpz",
                "import utils.strings\n\nlet line = strings.shout(strings.greeting)\nprint(line)\n",
            ),
        ],
        "main.tpz",
    )
    .expect("runs");
    assert_eq!(stdout, vec!["hello!"]);
}

#[test]
fn typed_json_materializes_root_alias_schemas() {
    let (value, _) = run_unit(
        &[(
            "main.tpz",
            "type Scalar<T> = T\ntype Lookup = Map<string, int>\nrecord Box<T> { value: Scalar<T>, lookup: Lookup }\nmatch JSON.parseAs<Box<int>>(\"\\{\\\"value\\\":7,\\\"lookup\\\":\\{\\\"answer\\\":8\\}\\}\") {\ncase Ok(boxed) => boxed.value + boxed.lookup.getOr(\"answer\", 0)\ncase Err(e) => 0\n}\n",
        )],
        "main.tpz",
    )
    .expect("typed JSON aliases run");
    assert_eq!(value, "15");
}

#[test]
fn typed_json_uses_the_imported_closures_declaration_scope() {
    let (value, _) = run_unit(
        &[
            (
                "lib.tpz",
                "type Scalar = int\nexport function decodeLocal() -> int {\n    match JSON.parseAs<Scalar>(\"7\") {\n    case Ok(value) => value\n    case Err(_) => 0\n    }\n}\n",
            ),
            (
                "main.tpz",
                "import lib\ntype Scalar = string\nlib.decodeLocal()\n",
            ),
        ],
        "main.tpz",
    )
    .expect("typed JSON keeps the defining module's alias scope");
    assert_eq!(value, "7");
}

#[test]
fn v520_typed_json_resolves_selected_qualified_and_nested_imported_schemas() {
    let (_, stdout) = run_unit_with_version(
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
            (
                "main.tpz",
                "import model\nimport selected { Box, UserAlias }\nlet qualified = JSON.parseAs<model.User>(\"\\{\\\"name\\\":\\\"Ada\\\"\\}\")\nlet aliased = JSON.parseAs<UserAlias>(\"\\{\\\"name\\\":\\\"Bea\\\"\\}\")\nlet generic = JSON.parseAs<Box<int>>(\"\\{\\\"value\\\":7,\\\"rank\\\":8\\}\")\nmatch qualified {\ncase Ok(user) => print(user.name)\ncase Err(error) => print(error)\n}\nmatch aliased {\ncase Ok(user) => print(user.name)\ncase Err(error) => print(error)\n}\nmatch generic {\ncase Ok(boxed) => print(\"{boxed.value + boxed.rank}\")\ncase Err(error) => print(error)\n}\n",
            ),
        ],
        "main.tpz",
        LangVersion::V5_20,
    )
    .expect("5.20 imported typed JSON schemas run");
    assert_eq!(stdout, vec!["Ada", "Bea", "15"]);
}

#[test]
fn v520_same_spelled_nominals_keep_module_identity_in_values_keys_and_patterns() {
    let (_, stdout) = run_unit_with_version(
        &[
            ("alpha.tpz", "export record User { id: int }\n"),
            ("beta.tpz", "export record User { id: int }\n"),
            (
                "main.tpz",
                "import alpha { User as AlphaUser }\nimport beta { User as BetaUser }\nlet alpha = AlphaUser { id: 1 }\nlet beta = BetaUser { id: 1 }\nprint(\"{alpha == beta}\")\nlet users = Set.of(alpha, beta)\nprint(\"{users.length}\")\nlet mut labels = Map.new()\nlabels.insert(alpha, \"alpha\")\nlabels.insert(beta, \"beta\")\nprint(\"{labels.length}\")\nmatch alpha {\ncase BetaUser { id } => print(\"wrong:{id}\")\ncase AlphaUser { id } => print(\"alpha:{id}\")\n}\n",
            ),
        ],
        "main.tpz",
        LangVersion::V5_20,
    )
    .expect("5.20 module-stable nominal values run");
    assert_eq!(stdout, vec!["false", "2", "2", "alpha:1"]);
}

#[test]
fn v520_private_qualified_nominals_do_not_cross_module_boundary() {
    let cases = [
        (
            "record",
            "record Hidden { value: int }\nexport let hidden = Hidden { value: 7 }\n",
        ),
        (
            "enum",
            "enum Hidden { Value }\nexport let hidden = Hidden.Value\n",
        ),
        (
            "newtype",
            "newtype Hidden = int\nexport let hidden = Hidden(7)\n",
        ),
    ];
    for (kind, module) in cases {
        let mut provider = InMemoryProvider::new();
        provider.add_file("lib.tpz", module);
        provider.add_file(
            "main.tpz",
            "import lib\nmatch lib.hidden {\ncase found: lib.Hidden => 1\ncase _ => 0\n}\n",
        );
        let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_20);
        assert!(
            unit.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.as_str() == "TPZ3009"),
            "{kind}"
        );
        let error = Machine::run_unit(&unit, &TestHost::new())
            .map(|value| render(&value))
            .map_err(|error| format!("{}: {}", error.code, error.message))
            .expect_err(kind);
        assert_eq!(
            error, "TPZ5001: `Hidden` is not an exported type of `lib` (§17)",
            "{kind}"
        );
    }
}

#[test]
fn record_default_unwind_does_not_leak_private_namespace_access() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "model.tpz",
        "let secret = 41\nfunction fail() -> Result<int, string> {\n    Err(\"default\")\n}\nexport record User { value: int = fail()? }\nexport function construct() -> Result<User, string> {\n    Ok(User {})\n}\n",
    );
    provider.add_file(
        "main.tpz",
        "import model\nlet attempted = model.construct()\nmodel.secret\n",
    );
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_20);
    assert!(
        unit.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "TPZ3009"),
        "private access must remain statically rejected"
    );
    let error = Machine::run_unit(&unit, &TestHost::new())
        .map(|value| render(&value))
        .map_err(|error| format!("{}: {}", error.code, error.message))
        .expect_err("record-default unwind must not retain private access authority");
    assert_eq!(error, "TPZ5001: `secret` is not exported by `model` (§17)");
}

#[test]
fn v520_bare_qualified_generic_nominal_keeps_runtime_arity_admission() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "lib.tpz",
        "export newtype Box<T> = T\nexport let value: Box<int> = Box(7)\n",
    );
    provider.add_file(
        "main.tpz",
        "import lib\nmatch lib.value {\ncase found: lib.Box => 1\ncase _ => 0\n}\n",
    );
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_20);
    assert!(unit.diagnostics.is_empty(), "{:?}", unit.diagnostics);
    let error =
        Machine::run_unit(&unit, &TestHost::new()).expect_err("missing type argument faults");
    assert_eq!(error.code, "TPZ5099");
    assert_eq!(
        error.message,
        "generic nominal type takes 1 type argument(s), found 0"
    );
}

#[test]
fn v520_bare_named_generic_nominals_keep_runtime_arity_admission() {
    let local_cases = [
        (
            "local record",
            "record Box<T> { value: T }\nlet value: Box<int> = Box { value: 7 }\nmatch value {\ncase found: Box => 1\ncase _ => 0\n}\n",
        ),
        (
            "local enum",
            "enum Box<T> { Value(T) }\nlet value: Box<int> = Box.Value(7)\nmatch value {\ncase found: Box => 1\ncase _ => 0\n}\n",
        ),
        (
            "local newtype",
            "newtype Box<T> = T\nlet value: Box<int> = Box(7)\nmatch value {\ncase found: Box => 1\ncase _ => 0\n}\n",
        ),
    ];
    for (kind, source) in local_cases {
        let error = run_unit_with_version(&[("main.tpz", source)], "main.tpz", LangVersion::V5_20)
            .expect_err(kind);
        assert_eq!(
            error, "TPZ5099: generic nominal type takes 1 type argument(s), found 0",
            "{kind}"
        );
    }

    let selected_cases = [
        (
            "selected record",
            "export record Box<T> { value: T }\nexport let value: Box<int> = Box { value: 7 }\n",
        ),
        (
            "selected enum",
            "export enum Box<T> { Value(T) }\nexport let value: Box<int> = Box.Value(7)\n",
        ),
        (
            "selected newtype",
            "export newtype Box<T> = T\nexport let value: Box<int> = Box(7)\n",
        ),
    ];
    for (kind, module) in selected_cases {
        let error = run_unit_with_version(
            &[
                ("lib.tpz", module),
                (
                    "main.tpz",
                    "import lib { Box as ForeignBox, value }\nmatch value {\ncase found: ForeignBox => 1\ncase _ => 0\n}\n",
                ),
            ],
            "main.tpz",
            LangVersion::V5_20,
        )
        .expect_err(kind);
        assert_eq!(
            error, "TPZ5099: generic nominal type takes 1 type argument(s), found 0",
            "{kind}"
        );
    }
}

#[test]
fn form_b_and_alias_bindings() {
    let (_, stdout) = run_unit(
        &[
            (
                "lib.tpz",
                "export function inc(x: int) -> int { return x + 1 }\nexport let base = 10\n",
            ),
            (
                "main.tpz",
                "import lib { inc as bump, base }\nprint(\"{bump(base)}\")\n",
            ),
        ],
        "main.tpz",
    )
    .expect("runs");
    assert_eq!(stdout, vec!["11"]);
}

#[test]
fn selected_generic_newtype_typed_pattern_uses_defining_base_type() {
    let (value, _) = run_unit(
        &[
            ("model.tpz", "export newtype Box<T> = T\n"),
            (
                "main.tpz",
                "import model { Box as ForeignBox }\nlet value: ForeignBox<int> = ForeignBox(7)\nmatch value {\ncase found: ForeignBox<int> => found.value()\ncase _ => 0\n}\n",
            ),
        ],
        "main.tpz",
    )
    .expect("selected generic newtype typed pattern runs");
    assert_eq!(value, "7");
}

#[test]
fn init_order_is_normative_and_eager() {
    // b imports a; a initializes first; entry last.
    let (_, stdout) = run_unit(
        &[
            (
                "a.tpz",
                "export let v = 1\nexport function side() -> int {\n    return 1\n}\n",
            ),
            ("b.tpz", "import a\nexport let w = a.v + 1\n"),
            ("main.tpz", "import b\nprint(\"{b.w}\")\n"),
        ],
        "main.tpz",
    )
    .expect("runs");
    assert_eq!(stdout, vec!["2"]);
}

#[test]
fn init_fault_aborts_with_module_context() {
    let err = run_unit(
        &[
            ("bad.tpz", "export let v = 1 / 0\n"),
            ("main.tpz", "import bad\nprint(\"{bad.v}\")\n"),
        ],
        "main.tpz",
    )
    .expect_err("init fault");
    assert!(err.starts_with("TPZ4002"), "{err}");
    assert!(err.contains("module `bad`"), "{err}");
}

#[test]
fn cross_module_closure_calls_swap_sources() {
    // The helper's body spans index into lib's source; calling it
    // from main must evaluate against lib's text.
    let (_, stdout) = run_unit(
        &[
            (
                "fmt/text.tpz",
                "export function wrap(inner: string) -> string {\n    let decorated = \"[\" + inner + \"]\"\n    return decorated\n}\n",
            ),
            (
                "main.tpz",
                "import fmt.text as t\nprint(t.wrap(\"x\"))\n",
            ),
        ],
        "main.tpz",
    )
    .expect("runs");
    assert_eq!(stdout, vec!["[x]"]);
}

#[test]
fn manual_forwarding_works() {
    let (_, stdout) = run_unit(
        &[
            ("core.tpz", "export let answer = 42\n"),
            (
                "facade.tpz",
                "import core\nexport let answer = core.answer\n",
            ),
            ("main.tpz", "import facade\nprint(\"{facade.answer}\")\n"),
        ],
        "main.tpz",
    )
    .expect("runs");
    assert_eq!(stdout, vec!["42"]);
}

#[test]
fn concurrent_arms_isolate_module_sources() {
    // One arm suspends inside an imported function (its src view)
    // while the sibling keeps evaluating entry-module spans.
    let (_, stdout) = run_unit(
        &[
            (
                "lib.tpz",
                "export function work(n: int) -> int {\n    let mut acc = 0\n    let mut i = 0\n    while i < n {\n        acc += i\n        i += 1\n    }\n    return acc\n}\n",
            ),
            (
                "main.tpz",
                "import lib\nlet r = concurrent {\n    remote: lib.work(500)\n    local: {\n        let mut here = 0\n        let mut j = 0\n        while j < 500 {\n            here += j\n            j += 1\n        }\n        here\n    }\n}\nprint(\"{r.remote} {r.local}\")\n",
            ),
        ],
        "main.tpz",
    )
    .expect("runs");
    assert_eq!(stdout, vec!["124750 124750"]);
}

#[test]
fn concurrent_arms_isolate_comprehension_accumulators() {
    let (value, _) = run_unit_with_version(
        &[(
            "main.tpz",
            "let r = concurrent {\n    left: [ for x in 0..<200 => x ]\n    right: [ for x in 1000..<1200 => x ]\n}\n\"{r.left.length}:{r.left[0]}:{r.left[199]}:{r.right.length}:{r.right[0]}:{r.right[199]}\"\n",
        )],
        "main.tpz",
        LangVersion::V5_20,
    )
    .expect("concurrent comprehensions run");
    assert_eq!(value, "200:0:199:200:1000:1199");
}

#[test]
fn concurrent_arms_isolate_record_default_private_authority() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "model.tpz",
        "let secret = 41\nfunction slow() -> int {\n    let mut i = 0\n    while i < 500 { i += 1 }\n    7\n}\nexport record User { value: int = slow() }\nexport function construct() -> User {\n    User {}\n}\n",
    );
    provider.add_file(
        "main.tpz",
        "import model\nlet r = concurrent {\n    defaulting: model.construct()\n    probe: model.secret\n}\nr.probe\n",
    );
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_20);
    assert!(
        unit.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "TPZ3009"),
        "private access must remain statically rejected"
    );
    let error = Machine::run_unit(&unit, &TestHost::new())
        .map(|value| render(&value))
        .map_err(|error| format!("{}: {}", error.code, error.message))
        .expect_err("a sibling arm must not inherit record-default private authority");
    assert_eq!(error, "TPZ5001: `secret` is not exported by `model` (§17)");
}

#[test]
fn concurrent_arms_inherit_enclosing_record_default_private_authority() {
    let (value, _) = run_unit_with_version(
        &[
            (
                "dependency.tpz",
                "let secret = 41\nexport let marker = 0\n",
            ),
            (
                "model.tpz",
                "import dependency\nexport record User { value: int = concurrent {\n    read: dependency.secret\n}.read }\n",
            ),
            (
                "main.tpz",
                "import model { User }\nlet user = User {}\nuser.value\n",
            ),
        ],
        "main.tpz",
        LangVersion::V5_20,
    )
    .expect("record-default arms retain their enclosing private authority");
    assert_eq!(value, "41");
}

#[test]
fn faulted_deferred_comprehension_restores_enclosing_accumulator() {
    let (value, _) = run_unit_with_version(
        &[(
            "main.tpz",
            "function item() -> int {\n    defer { [ for y in 0..<2 => 1 / 0 ] }\n    7\n}\n[ for x in 0..<3 => item() ]\n",
        )],
        "main.tpz",
        LangVersion::V5_20,
    )
    .expect("a contained deferred fault must not corrupt its enclosing comprehension");
    assert_eq!(value, "[7, 7, 7]");
}

#[test]
fn faulted_deferred_record_default_revokes_private_authority() {
    let mut provider = InMemoryProvider::new();
    provider.add_file(
        "model.tpz",
        "let secret = 41\nrecord Broken { value: int = 1 / 0 }\nexport function construct() -> Broken { Broken {} }\n",
    );
    provider.add_file(
        "main.tpz",
        "import model\nfunction run() -> int {\n    defer model.construct()\n    0\n}\nrun()\nmodel.secret\n",
    );
    let unit = resolve_with_version(&provider, "main.tpz", None, LangVersion::V5_20);
    assert!(
        unit.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "TPZ3009"),
        "private access must remain statically rejected"
    );
    let host = TestHost::new();
    let error = Machine::run_unit(&unit, &host)
        .map(|value| render(&value))
        .map_err(|error| format!("{}: {}", error.code, error.message))
        .expect_err("a contained deferred fault must revoke record-default authority");
    assert_eq!(
        host.defer_errors(),
        vec!["TPZ4002: integer division by zero"]
    );
    assert_eq!(error, "TPZ5001: `secret` is not exported by `model` (§17)");
}

#[test]
fn const_evaluates_at_load_time() {
    let (_, stdout) = run_unit(
        &[
            (
                "cfg.tpz",
                "export const limit = 4 * 25\nexport const label = \"max\"\n",
            ),
            (
                "main.tpz",
                "import cfg\nprint(\"{cfg.label}: {cfg.limit}\")\n",
            ),
        ],
        "main.tpz",
    )
    .expect("runs");
    assert_eq!(stdout, vec!["max: 100"]);
    // Non-const initializers guard at load, before any host effect.
    let err = run_unit(
        &[
            ("bad.tpz", "export const c = print(\"side\")\n"),
            ("main.tpz", "import bad\nprint(\"x\")\n"),
        ],
        "main.tpz",
    )
    .expect_err("non-const guard");
    assert!(err.starts_with("TPZ5001"), "{err}");
    // Constant faults are static errors, not runtime faults (§13a).
    let err2 = run_unit(
        &[
            ("bad.tpz", "export const c = 1 / 0\n"),
            ("main.tpz", "import bad\nprint(\"x\")\n"),
        ],
        "main.tpz",
    )
    .expect_err("const fault guard");
    assert!(err2.starts_with("TPZ5001"), "{err2}");
}

#[test]
fn init_fault_carries_the_import_chain() {
    let err = run_unit(
        &[
            ("a.tpz", "import b\nexport let v = b.w\n"),
            ("b.tpz", "export let w = 1 / 0\n"),
            ("main.tpz", "import a\nprint(\"{a.v}\")\n"),
        ],
        "main.tpz",
    )
    .expect_err("init fault");
    assert!(err.contains("import chain: main -> a -> b"), "{err}");
}
