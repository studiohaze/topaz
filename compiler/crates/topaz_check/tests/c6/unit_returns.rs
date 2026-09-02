use super::imports_and_suggestions::STRINGS;
use super::*;

// ---- TPZ5002 graduation: the unit name space is closed -----------------------

#[test]
fn unbound_names_are_tpz5002() {
    assert_code(&[("main", "let x = mystery\nprint(\"{x}\")")], "TPZ5002");
    assert_code(
        &[("main", "let x = loadUser(42)\nprint(\"{x}\")")],
        "TPZ5002",
    );
    assert_code(&[("main", "let rows = YAML.parse(\"x\")")], "TPZ5002");
}

#[test]
fn import_aliases_replace_the_original_names() {
    // Form A: the alias is the only bound name.
    assert_code(
        &[
            ("utils.strings", STRINGS),
            (
                "main",
                "import utils.strings as text\nlet t: string = text.trim(\"x\")\nlet bad = strings\nprint(\"{bad}\")",
            ),
        ],
        "TPZ5002",
    );
    // Form B: the selection alias is the only bound name.
    assert_code(
        &[
            ("utils.strings", STRINGS),
            (
                "main",
                "import utils.strings { trim as cut }\nlet t: string = cut(\"x\")\nlet bad = trim(\"x\")\nprint(\"{bad}\")",
            ),
        ],
        "TPZ5002",
    );
}

#[test]
fn dotted_expressions_are_not_namespace_paths() {
    // `utils` is not bound by `import utils.strings`.
    assert_code(
        &[
            ("utils.strings", STRINGS),
            (
                "main",
                "import utils.strings\nlet t = utils.strings.trim(\"x\")\nprint(\"{t}\")",
            ),
        ],
        "TPZ5002",
    );
}

#[test]
fn non_exports_through_a_namespace_are_tpz5002() {
    assert_code(
        &[
            ("utils.strings", STRINGS),
            (
                "main",
                "import utils.strings\nlet t = strings.nope\nprint(\"{t}\")",
            ),
        ],
        "TPZ5002",
    );
    assert_code(
        &[
            ("utils.strings", STRINGS),
            ("main", "import utils.strings { nope }\nprint(\"x\")"),
        ],
        "TPZ5002",
    );
}

#[test]
fn the_prelude_stays_open_in_module_mode() {
    assert_clean(&[(
        "main",
        "let xs: Array<int> = map(1..3, (x: int) => x * 2)\nlet m: Map<string, int> = Map.new()\nlet n: Option<int> = None\nprint(\"{xs} {m} {n}\")",
    )]);
}

// ---- unit mechanics -----------------------------------------------------------

#[test]
fn dependency_order_is_computed_not_given() {
    // The importer comes FIRST in the given order; the checker must
    // still see the library's exports.
    assert_clean(&[
        (
            "main",
            "import utils.strings { trim }\nlet t: string = trim(\"x\")\nprint(t)",
        ),
        ("utils.strings", STRINGS),
    ]);
}

#[test]
fn local_bindings_shadow_imports_silently() {
    // A module top-level name shadows a prelude name (ADR-084);
    // import-vs-local collisions are resolver territory. Here the
    // local function simply wins lookup.
    assert_clean(&[(
        "main",
        "function double(n: int) -> int {\n    return n * 2\n}\nlet x: int = double(2)\nprint(\"{x}\")",
    )]);
}

// ---- review fold (r1) -------------------------------------------------------

#[test]
fn omitted_returns_infer_and_export() {
    // The r1 counterexample: an exported omitted-return function
    // carries its inferred return type to importers.
    assert_code(
        &[
            ("lib", "export function answer() {\n    return 42\n}\n"),
            (
                "main",
                "import lib { answer }\nlet s: string = answer()\nprint(s)",
            ),
        ],
        "TPZ5001",
    );
    assert_clean(&[
        ("lib", "export function answer() {\n    return 42\n}\n"),
        (
            "main",
            "import lib { answer }\nlet n: int = answer()\nprint(\"{n}\")",
        ),
    ]);
    // Locally too, after the declaration.
    assert_code(
        &[(
            "main",
            "function answer() {\n    return 42\n}\nlet s: string = answer()\nprint(s)",
        )],
        "TPZ5001",
    );
}

#[test]
fn namespace_bindings_shadow_static_heads() {
    // ADR-084: a namespace bound as `Map` wins over the builtin
    // static head, so `Map.new()` is the exported string function.
    assert_code(
        &[
            (
                "utils.collections",
                "export function new() -> string {\n    return \"not a map\"\n}\n",
            ),
            (
                "main",
                "import utils.collections as Map\nlet m: Map<string, int> = Map.new()\nprint(\"{m}\")",
            ),
        ],
        "TPZ5001",
    );
}

#[test]
fn exported_const_callables_keep_their_arity() {
    assert_code(
        &[
            ("lib", "export const id = (x: int) => x\n"),
            (
                "main",
                "import lib { id }\nlet n: int = id()\nprint(\"{n}\")",
            ),
        ],
        "TPZ5004",
    );
    assert_clean(&[
        ("lib", "export const id = (x: int) => x\n"),
        (
            "main",
            "import lib { id }\nlet n: int = id(3)\nprint(\"{n}\")",
        ),
    ]);
}

#[test]
fn reexported_functions_keep_callable_metadata() {
    // `export let g = greet` forwards the default-parameter arity.
    assert_clean(&[
        (
            "a",
            "export function greet(name: string, suffix: string = \"!\") -> string {\n    return \"{name}{suffix}\"\n}\n",
        ),
        ("b", "import a { greet }\nexport let g = greet\n"),
        (
            "main",
            "import b { g }\nlet s: string = g(\"Topaz\")\nprint(s)",
        ),
    ]);
}

// ---- review fold (r2) -------------------------------------------------------

#[test]
fn lambda_returns_belong_to_the_lambda() {
    // §5/§7: `return` binds to the innermost function or lambda, so
    // a lambda's return must not pollute the enclosing function's
    // inferred return type.
    assert_clean(&[(
        "main",
        "function outer() {\n    let f = (x: int) => {\n        return x * 2\n    }\n    return \"done: {f(2)}\"\n}\nlet s: string = outer()\nprint(s)",
    )]);
    // And the lambda's own type joins its returns.
    assert_code(
        &[(
            "main",
            "let f = (x: int) => {\n    return x * 2\n}\nlet s: string = f(2)\nprint(s)",
        )],
        "TPZ5001",
    );
}

#[test]
fn recursive_omitted_returns_are_static_errors() {
    // CDR-004 §7: recursive and mutually recursive functions
    // require a declared return type.
    assert_code(
        &[(
            "main",
            "function fact(n: int) {\n    if n <= 1 {\n        return 1\n    }\n    return n * fact(n - 1)\n}\nprint(\"{fact(5)}\")",
        )],
        "TPZ5022",
    );
    assert_code(
        &[(
            "main",
            "function ping(n: int) {\n    if n <= 0 {\n        return 0\n    }\n    return pong(n - 1)\n}\nfunction pong(n: int) {\n    return ping(n - 1)\n}\nprint(\"{ping(3)}\")",
        )],
        "TPZ5022",
    );
    // A declared return type makes the same shape legal.
    assert_clean(&[(
        "main",
        "function fact(n: int) -> int {\n    if n <= 1 {\n        return 1\n    }\n    return n * fact(n - 1)\n}\nprint(\"{fact(5)}\")",
    )]);
}

#[test]
fn wrapped_aliases_keep_callable_metadata() {
    assert_clean(&[
        (
            "a",
            "export function greet(name: string, suffix: string = \"!\") -> string {\n    return \"{name}{suffix}\"\n}\n",
        ),
        ("b", "import a { greet }\nexport let g = (greet)\n"),
        (
            "main",
            "import b { g }\nlet s: string = g(\"Topaz\")\nprint(s)",
        ),
    ]);
}

// ---- review fold (r3) -------------------------------------------------------

#[test]
fn nested_same_name_functions_keep_the_pending_marker() {
    // The r3 counterexample: a nested `f` must not clear the
    // pending-return marker for the later top-level recursive `f`.
    assert_code(
        &[(
            "main",
            "function g() {\n    function f() {\n        return 0\n    }\n    return 0\n}\nfunction f(n: int) {\n    if n <= 0 {\n        return 0\n    }\n    return n * f(n - 1)\n}\nprint(\"{f(3)}\")",
        )],
        "TPZ5022",
    );
}

#[test]
fn lambda_return_partials_complete_each_other() {
    // The r3 counterexample: lambda returns of Ok/Err mutually
    // complete (§22.1) and a wrong annotation downstream is caught.
    assert_code(
        &[(
            "main",
            "let b: bool = true\nlet f = () => if b {\n    return Ok(1)\n} else {\n    return Err(\"e\")\n}\nlet bad: Result<string, int> = f()",
        )],
        "TPZ5001",
    );
    assert_clean(&[(
        "main",
        "let b: bool = true\nlet f = () => if b {\n    return Ok(1)\n} else {\n    return Err(\"e\")\n}\nlet good: Result<int, string> = f()",
    )]);
}

#[test]
fn omitted_returns_solve_ok_err_pairs() {
    assert_clean(&[(
        "main",
        "function pick(b: bool) {\n    if b {\n        return Ok(1)\n    }\n    return Err(\"e\")\n}\nlet r: Result<int, string> = pick(true)\nprint(\"{r}\")",
    )]);
}

// ---- review fold (r4) -------------------------------------------------------

#[test]
fn completed_shadowing_functions_do_not_leak_taint() {
    // The r4 counterexample: g's call resolves to the COMPLETED
    // nested f, so the later pending top-level f must not make g
    // "recursive".
    assert_clean(&[(
        "main",
        "function g() {\n    function f() {\n        return 0\n    }\n    let n = f()\n    return n\n}\nfunction f(n: int) {\n    return n\n}\nprint(\"{g()} {f(1)}\")",
    )]);
}

// ---- review fold (r5) -------------------------------------------------------

#[test]
fn pending_taint_follows_aliases() {
    // The r5 counterexample: `let f = f` aliases the pending
    // top-level f; calling through the alias still taints, so the
    // mutual recursion is TPZ5022.
    assert_code(
        &[(
            "main",
            "function g() {\n    let f = f\n    return f()\n}\nfunction f() {\n    return g()\n}\nprint(\"{g()}\")",
        )],
        "TPZ5022",
    );
}

// ---- review fold (r6) -------------------------------------------------------

#[test]
fn alias_taint_clears_when_the_source_completes() {
    // The r6 counterexample: g aliases f; once f's body completes,
    // calls through g are not treated as recursive (TPZ taint clears).
    //
    // f is declared BEFORE `let g = f`: capturing f at the top level
    // requires it to already be bound, so the earlier `let g = f \n
    // function f ...` ordering was itself a §4 forward-initializer fault
    // (an immediate capture of a textually-later binding — the
    // interpreter faults `f is not bound`).
    assert_clean(&[(
        "main",
        "function f() {\n    return 0\n}\nlet g = f\nfunction h() {\n    return g()\n}\nprint(\"{h()}\")",
    )]);
}

#[test]
fn imported_lambda_named_arguments_stay_unjudged() {
    // An exported lambda carries no authoritative parameter-name
    // table; the consumer's named call stays staged, not TPZ5004.
    assert_clean(&[
        (
            "main",
            "import lib { id }\nlet n: int = id(x: 1)\nprint(\"{n}\")",
        ),
        ("lib", "export const id = (x: int) => x"),
    ]);
}
