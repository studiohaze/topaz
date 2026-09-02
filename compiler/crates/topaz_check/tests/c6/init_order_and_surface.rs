use super::*;

// ---- top-level init order (§4): ONLY a top-level non-function
//      statement's OWN, immediately-evaluated forward reference is a
//      static error (TPZ5002). A function/lambda/defer body that NAMES
//      a later binding is §4-ALLOWED (mutual recursion); calling such a
//      body before the name is bound is a DYNAMIC runtime fault `check`
//      is not obligated to catch (like `1/0`). The rule is purely
//      SYNTACTIC over the statement's own immediate expression; it
//      never follows a call into another body. ----

#[test]
fn nominal_record_pattern_binding_shadows_a_later_top_level_name() {
    let output = check_output_with_version(
        &[(
            "main",
            "record Point { x: int }\n\
             let observed = {\n\
                 let Point { x } = Point { x: 1 }\n\
                 x\n\
             }\n\
             let x = 2\n\
             print(\"{observed}\")",
        )],
        LangVersion::V5_4,
    );
    assert!(
        output.diagnostics.is_empty(),
        "the block-local nominal-record binding must shadow the later top-level name: {:?}",
        output.diagnostics
    );
}

/// A `const` is bound by the load-time const pass before any runtime
/// statement, so a runtime binding or a function body may read a
/// textually-LATER const — type visibility AND runtime availability.
#[test]
fn a_later_const_hoists_for_statements_and_bodies() {
    // `let` initializer reads a later const.
    assert_clean(&[("main", "let x = N\nconst N = 7\nprint(\"{x}\")")]);
    // A top-level statement reads a later const.
    assert_clean(&[("main", "print(\"{N}\")\nconst N = 5")]);
    // A function BODY reads a later const (resolved when called).
    assert_clean(&[(
        "main",
        "function g() -> int { N }\nconst N = 10\nlet r = g()\nprint(\"{r}\")",
    )]);
}

/// A function body resolves names at CALL time, so a body may read a
/// runtime binding declared textually later. §4 does not scan bodies,
/// so this is clean (calling it before `y` is bound would be a dynamic
/// fault, not a static error).
#[test]
fn a_body_resolves_a_later_let_at_call_time() {
    assert_clean(&[(
        "main",
        "function g() -> int { y }\nlet y = 5\nlet r = g()\nprint(\"{r}\")",
    )]);
}

/// A forward FUNCTION reference in a top-level statement's OWN
/// immediately-evaluated expression (a `let` initializer or an
/// expression statement) is a static error — the function is not bound
/// until its declaration runs.
#[test]
fn a_forward_function_reference_from_a_statement_is_tpz5002() {
    // From a `let` initializer.
    assert_code(
        &[(
            "main",
            "let x = f()\nfunction f() -> int { 5 }\nprint(\"{x}\")",
        )],
        "TPZ5002",
    );
    // From a string-interpolation expression in a statement.
    assert_code(
        &[("main", "print(\"{f()}\")\nfunction f() -> int { 5 }")],
        "TPZ5002",
    );
}

/// A forward reference reached only THROUGH A CALL into another body is
/// §4-ALLOWED: `a` is bound (index 0); we do NOT scan `a`'s body, so
/// `a`'s call of the textually-later `b` is not a static error. (At
/// runtime `b` is bound before `a()` runs here, so it is also a clean
/// PROGRAM; even were it not, §4 would still allow it statically.)
#[test]
fn a_transitive_forward_call_is_section4_allowed() {
    assert_clean(&[(
        "main",
        "function a() -> int { b() }\nlet x = a()\nfunction b() -> int { 9 }\nprint(\"{x}\")",
    )]);
}

/// Immediately-evaluated SUBexpressions of a statement count: an array
/// element, a record field value, and a pipe rhs are all evaluated when
/// the statement runs, so a forward reference there is a static error.
#[test]
fn forward_refs_in_immediate_subexpressions_are_tpz5002() {
    // Array element.
    assert_code(
        &[(
            "main",
            "let xs = [f()]\nfunction f() -> int { 5 }\nprint(\"{xs}\")",
        )],
        "TPZ5002",
    );
    // Record field value.
    assert_code(
        &[(
            "main",
            "let r = { v: f() }\nfunction f() -> int { 5 }\nprint(\"{r}\")",
        )],
        "TPZ5002",
    );
    // Pipe rhs (the immediate stage).
    assert_code(
        &[(
            "main",
            "let x = 0 |> addOne\nfunction addOne(n: int) -> int { n + 1 }\nprint(\"{x}\")",
        )],
        "TPZ5002",
    );
}

/// A reference to a later binding inside a DELAYED position — a lambda
/// body (IIFE or stored), a `defer` body, a conditional branch, a
/// short-circuit RHS, or optional-call arguments — is §4-ALLOWED, even
/// though calling/running it early would fault at runtime. `check` is
/// not obligated to catch the dynamic fault.
#[test]
fn forward_refs_in_delayed_positions_are_section4_allowed() {
    // Immediately-invoked lambda body (the body is delayed).
    assert_clean(&[(
        "main",
        "let x = (() => f())()\nfunction f() -> int { 5 }\nprint(\"{x}\")",
    )]);
    // Stored lambda called early (the body is delayed).
    assert_clean(&[(
        "main",
        "let g = () => f()\nlet y = g()\nfunction f() -> int { 5 }\nprint(\"{y}\")",
    )]);
    // `defer` body (runs at scope exit).
    assert_clean(&[("main", "defer print(\"{f()}\")\nfunction f() -> int { 5 }")]);
    // An `if` branch not taken.
    assert_clean(&[(
        "main",
        "let z = if false { f() } else { 0 }\nfunction f() -> int { 5 }\nprint(\"{z}\")",
    )]);
}

#[test]
fn short_circuit_rhs_and_optional_call_arguments_are_delayed_for_init_order() {
    for source in [
        "let observed: bool = false && later\nlet later: bool = true\nprint(\"{observed}\")",
        "let observed: bool = true || later\nlet later: bool = false\nprint(\"{observed}\")",
        "let present: Option<int> = Some(1)\nlet observed: int = present ?? later\nlet later: int = 2\nprint(\"{observed}\")",
        "let receiver: Option<string> = None\nlet observed: string = receiver?.replace(later, \"y\") ?? \"skipped\"\nlet later: string = \"x\"\nprint(observed)",
    ] {
        assert_clean(&[("main", source)]);
    }
}

/// Mutual recursion CALLED after both functions are defined is fine:
/// every callee is bound by the time the call runs, and §4 never scans
/// either body anyway.
#[test]
fn mutual_recursion_called_after_both_definitions_is_clean() {
    assert_clean(&[(
        "main",
        "function even(n: int) -> bool { if n == 0 { true } else { odd(n - 1) } }\n\
         function odd(n: int) -> bool { if n == 0 { false } else { even(n - 1) } }\n\
         let r = even(4)\nprint(\"{r}\")",
    )]);
}

/// A runtime binding reading a textually-later runtime binding in its
/// OWN initializer faults at load (no partially initialized binding) —
/// TPZ5002.
#[test]
fn a_let_reading_a_later_let_is_tpz5002() {
    assert_code(
        &[("main", "let x = y\nlet y = 5\nprint(\"{x}\")")],
        "TPZ5002",
    );
}

/// A `const` can only see EARLIER consts: reading a later const is a
/// load-time fault from the const pass (a SEPARATE rule from the §4
/// runtime forward scan), kept as a static error.
#[test]
fn a_const_reading_a_later_const_is_rejected() {
    assert_code(
        &[("main", "const B = A + 1\nconst A = 2\nprint(\"{B}\")")],
        "TPZ5002",
    );
}

/// Capturing a (bound) top-level function into a `let` and calling
/// through it is §4-ALLOWED regardless of where the captured function's
/// own callees are declared: the capture reads only `a` (already bound),
/// and §4 never follows the call into `a`'s body. Calling `h()` before
/// `b` is bound is a dynamic fault, not a static error.
#[test]
fn capturing_and_calling_a_bound_function_is_section4_allowed() {
    // `h` captures `a` (bound); `b` is declared before the call — also
    // a clean program.
    assert_clean(&[(
        "main",
        "function a() -> int { b() }\nlet h = a\nfunction b() -> int { 9 }\nlet r = h()\nprint(\"{r}\")",
    )]);
    // `h()` runs before `b` is bound: a DYNAMIC fault at runtime, but
    // §4-allowed statically (we never scan `a`'s body).
    assert_clean(&[(
        "main",
        "function a() -> int { b() }\nlet h = a\nlet r = h()\nfunction b() -> int { 9 }\nprint(\"{r}\")",
    )]);
}

// ---- C4: a member-CALL typo suggests only CALLABLE members ----------

#[test]
fn builtin_member_call_suggests_a_method_not_a_property() {
    // A call-position typo of a METHOD (`push`) is suggested…
    assert_message_contains(
        &[(
            "main",
            "let xs = [1, 2, 3]\nlet _ = xs.puhs(1)\nprint(\"x\")",
        )],
        "did you mean `push`?",
    );
    // …but a call-position typo near the `length` PROPERTY must NOT suggest it:
    // `xs.length()` is not callable (C4). Before C4 the call hint offered it.
    let diags = check(&[(
        "main",
        "let xs = [1, 2, 3]\nlet _ = xs.lenght()\nprint(\"x\")",
    )]);
    assert!(
        diags.iter().any(|d| d.starts_with("TPZ5006")),
        "expected a no-member diagnostic, got: {diags:?}"
    );
    assert!(
        !diags.iter().any(|d| d.contains("`length`")),
        "a member CALL must not suggest the non-callable `length` property: {diags:?}"
    );
}

#[test]
fn record_field_call_suggests_a_function_field_not_data() {
    // A record-field CALL typo suggests a FUNCTION-typed field…
    assert_message_contains(
        &[(
            "main",
            "let r = { runner: () => 1, flagged: true }\nlet _ = r.runer()\nprint(\"x\")",
        )],
        "did you mean `runner`?",
    );
    // …but never a plain DATA field — `flagged` is not callable (C4).
    let diags = check(&[(
        "main",
        "let r = { runner: () => 1, flagged: true }\nlet _ = r.flaged()\nprint(\"x\")",
    )]);
    assert!(
        diags.iter().any(|d| d.starts_with("TPZ5006")),
        "expected a no-field diagnostic, got: {diags:?}"
    );
    assert!(
        !diags.iter().any(|d| d.contains("`flagged`")),
        "a record-field CALL must not suggest the non-callable `flagged` data field: {diags:?}"
    );
}

// ---- projection must not escape the module surface --------------------------

#[test]
fn an_exported_value_must_not_publish_a_projection() {
    // Destructuring a Foreign / opaque value mints a rigid projection
    // (`FieldOf<Status, x>`) with no nameable spelling. It must not escape the
    // module surface: an exported binding that would publish one is rejected with
    // a request to annotate (TPZ5022) — the export-surface analogue of the
    // omitted-return leak guard, so a downstream importer never sees an
    // unnameable rigid type.
    assert_code(
        &[(
            "main",
            "export const f = (v: Status) => match v {\n    case { x } => x\n    case _ => v\n}",
        )],
        "TPZ5022",
    );
    // A non-exported (local) binding holding the same projection stays gradual: it
    // is contained, and using it at a concrete type would still reject on its own.
    let local = check_output_with_version(
        &[(
            "main",
            "let f = (v: Status) => match v {\n    case { x } => x\n    case _ => v\n}",
        )],
        LangVersion::V5_5,
    );
    assert!(
        local.diagnostics.is_empty(),
        "a local projection must stay contained: {:?}",
        local.diagnostics
    );
    let surface = &local.exports["main"];
    let private = &surface.private_runtime_values["f"].ty;
    assert!(
        !private.to_string().contains("FieldOf<"),
        "a module-local projection must not cross the private runtime surface: {private}"
    );
    assert!(
        surface.private_runtime_projection_tainted.contains("f"),
        "the gradualized private value must retain a fail-closed taint"
    );

    // A consumer must not treat that gradualized type as evidence that the
    // original projection satisfies a concrete record-field type. Fields always
    // have declared types, so there is no default-driven field-type inference
    // path to wash the projection into an exported record surface.
    let cross_module = check_output_with_version(
        &[
            (
                "main",
                "import config as cfg\nexport record Holder { f: (Status) -> Status = cfg.hidden }",
            ),
            (
                "config",
                "let hidden = (v: Status) => match v {\n    case { x } => x\n    case _ => v\n}\nexport const marker = 0",
            ),
        ],
        LangVersion::V5_5,
    );
    assert!(
        cross_module
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "TPZ5022"),
        "a cross-module private projection default must fail closed: {:?}",
        cross_module.diagnostics
    );
}
