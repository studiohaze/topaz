use super::*;

// ---- "did you mean …?" on TPZ5002 (unbound) + not-exported -----------------

#[test]
fn unbound_local_typo_suggests_the_name() {
    assert_message_contains(
        &[("main", "let length = 5\nlet y = lenght\nprint(\"{y}\")")],
        "is not bound; did you mean `length`?",
    );
}

#[test]
fn unbound_callee_typo_suggests_a_builtin() {
    assert_message_contains(&[("main", "prnt(\"hi\")")], "did you mean `print`?");
}

#[test]
fn not_exported_import_typo_suggests_the_export() {
    assert_message_contains(
        &[
            ("utils.strings", STRINGS),
            ("main", "import utils.strings { trmi }\nprint(\"x\")"),
        ],
        "did you mean `trim`?",
    );
}

#[test]
fn not_exported_member_typo_suggests_the_export() {
    assert_message_contains(
        &[
            ("utils.strings", STRINGS),
            ("main", "import utils.strings\nprint(\"{strings.greting}\")"),
        ],
        "did you mean `greeting`?",
    );
}

#[test]
fn unrelated_unbound_name_offers_no_suggestion() {
    let diags = check(&[("main", "let y = qqqqqq\nprint(\"{y}\")")]);
    assert!(
        diags
            .iter()
            .any(|d| d.contains("is not bound") && !d.contains("did you mean")),
        "want no suggestion, got: {diags:?}"
    );
}

#[test]
fn unbound_constant_typo_suggests_none() {
    assert_message_contains(
        &[("main", "let n = Noen\nprint(\"{n}\")")],
        "did you mean `None`?",
    );
}

#[test]
fn unbound_callee_suggests_only_callable_names() {
    // A callable local IS offered for a callee typo …
    assert_message_contains(
        &[(
            "main",
            "let myfunc = (x: int) => x\nlet y = myfnc(1)\nprint(\"{y}\")",
        )],
        "did you mean `myfunc`?",
    );
    // … but a non-callable local of a close name is NOT (you cannot call it).
    let diags = check(&[("main", "let myval = 5\nmyvl()")]);
    assert!(
        diags
            .iter()
            .any(|d| d.contains("is not bound") && !d.contains("did you mean")),
        "a non-callable local must not be offered for a callee typo, got: {diags:?}"
    );
    // The same non-callable local IS offered in value position.
    assert_message_contains(
        &[("main", "let myval = 5\nlet y = myvl\nprint(\"{y}\")")],
        "did you mean `myval`?",
    );
}

#[test]
fn unbound_value_does_not_suggest_a_namespace() {
    // A bare namespace is not a value (TPZ3012), so a value-position typo is never
    // offered an imported namespace name.
    let diags = check(&[
        ("utils.strings", STRINGS),
        (
            "main",
            "import utils.strings\nlet x = strngs\nprint(\"{x}\")",
        ),
    ]);
    assert!(
        diags
            .iter()
            .any(|d| d.contains("is not bound") && !d.contains("did you mean")),
        "a namespace must not be offered as a value suggestion, got: {diags:?}"
    );
}

#[test]
fn unbound_callee_respects_shadowing() {
    // A builtin free function shadowed by a non-callable local is NOT offered for
    // a callee typo: `print()` would resolve to the local `int`, not the builtin.
    let diags = check(&[("main", "let print = 1\nprnt(\"x\")")]);
    assert!(
        diags
            .iter()
            .any(|d| d.contains("is not bound") && !d.contains("did you mean")),
        "a shadowed builtin must not be offered for a callee typo, got: {diags:?}"
    );
}

#[test]
fn not_exported_import_alias_typo_suggests_the_alias() {
    // `User` is an exported type alias; a selected-import typo of it is suggested
    // from the alias surface, not just the value surface.
    assert_message_contains(
        &[
            ("utils.strings", STRINGS),
            ("main", "import utils.strings { Usre }\nprint(\"x\")"),
        ],
        "did you mean `User`?",
    );
}

pub(super) const STRINGS: &str = "export function trim(s: string) -> string {\n    return s\n}\nexport let greeting = \"hi\"\nexport const LIMIT = 10\nexport type User = { id: int }\nexport type Pair<T> = { a: T, b: T }\n";

// ---- exported signatures consumed by importers -----------------------------

#[test]
fn selected_imports_bind_and_type_check() {
    assert_clean(&[
        ("utils.strings", STRINGS),
        (
            "main",
            "import utils.strings { trim }\nlet t: string = trim(\"x\")\nprint(t)",
        ),
    ]);
    assert_code(
        &[
            ("utils.strings", STRINGS),
            (
                "main",
                "import utils.strings { trim }\nlet t = trim(1)\nprint(t)",
            ),
        ],
        "TPZ5001",
    );
    assert_code(
        &[
            ("utils.strings", STRINGS),
            (
                "main",
                "import utils.strings { trim }\nlet t = trim()\nprint(t)",
            ),
        ],
        "TPZ5004",
    );
}

#[test]
fn namespace_members_type_through_the_surface() {
    assert_clean(&[
        ("utils.strings", STRINGS),
        (
            "main",
            "import utils.strings\nlet t: string = strings.trim(strings.greeting)\nlet n: int = strings.LIMIT\nprint(\"{t} {n}\")",
        ),
    ]);
    assert_code(
        &[
            ("utils.strings", STRINGS),
            (
                "main",
                "import utils.strings\nlet t = strings.trim(1)\nprint(\"{t}\")",
            ),
        ],
        "TPZ5001",
    );
}

#[test]
fn optional_access_on_namespace_is_rejected_before_backends() {
    let diags = check(&[
        ("config", "export const base = 36\n"),
        (
            "main",
            "import config\nlet n = config?.base\nprint(\"{n}\")",
        ),
    ]);
    assert!(
        diags.iter().any(|diag| diag.starts_with("TPZ5001")),
        "expected TPZ5001, got: {diags:?}"
    );
    assert!(
        diags.iter().any(|diag| diag.contains("found `namespace`")),
        "expected namespace optional-access diagnostic, got: {diags:?}"
    );
}

#[test]
fn generic_exports_instantiate_per_call() {
    let lib = "export function first<T>(xs: Array<T>) -> Option<T> {\n    return xs.get(0)\n}\n";
    assert_clean(&[
        ("lib", lib),
        (
            "main",
            "import lib { first }\nlet x: Option<int> = first([1, 2])",
        ),
    ]);
    assert_code(
        &[
            ("lib", lib),
            (
                "main",
                "import lib { first }\nlet x: Option<string> = first([1, 2])",
            ),
        ],
        "TPZ5001",
    );
}

#[test]
fn exported_generic_protocol_bounds_are_enforced_by_importers() {
    let lib =
        "export function render<T: Show>(value: T) -> string {\n    return Show.show(value)\n}\n";
    let clean = check_output_with_version(
        &[
            ("lib", lib),
            (
                "main",
                "import lib { render }\nrecord User derives Show { name: string }\nlet s: string = render(User { name: \"Ada\" })\nprint(s)",
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        clean.diagnostics.is_empty(),
        "expected clean, got: {:?}",
        clean.diagnostics
    );
    let rejected = check_output_with_version(
        &[
            ("lib", lib),
            (
                "main",
                "import lib { render }\nrecord User { name: string }\nlet s = render(User { name: \"Ada\" })\nprint(s)",
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        rejected
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "TPZ5522"),
        "expected TPZ5522, got: {:?}",
        rejected.diagnostics
    );
}

#[test]
fn namespace_imported_generic_protocol_bounds_are_enforced() {
    let lib =
        "export function render<T: Show>(value: T) -> string {\n    return Show.show(value)\n}\n";
    let rejected = check_output_with_version(
        &[
            ("lib", lib),
            (
                "main",
                "import lib\nrecord User { name: string }\nlet s = lib.render(User { name: \"Ada\" })\nprint(s)",
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        rejected
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "TPZ5522"),
        "expected TPZ5522, got: {:?}",
        rejected.diagnostics
    );
}

#[test]
fn typed_json_imports_follow_the_profile_identity_boundary() {
    let legacy = check_output_with_version(
        &[
            (
                "main",
                "import model { User }\nlet value = JSON.parseAs<User>(\"null\")\nprint(\"{value}\")",
            ),
            ("model", "export record User { name: string }"),
        ],
        LangVersion::V5_19,
    );
    assert!(
        legacy
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "TPZ5534"),
        "expected compatibility-profile TPZ5534, got: {:?}",
        legacy.diagnostics
    );

    let output = check_output_with_version(
        &[
            (
                "main",
                "import model { User }\nlet value = JSON.parseAs<User>(\"null\")\nprint(\"{value}\")",
            ),
            ("model", "export record User { name: string }"),
        ],
        LangVersion::V5_20,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);

    let alias_output = check_output_with_version(
        &[
            (
                "main",
                "import model { UserAlias }\nlet value = JSON.parseAs<UserAlias>(\"null\")\nprint(\"{value}\")",
            ),
            (
                "model",
                "record User { name: string }\nexport type UserAlias = User",
            ),
        ],
        LangVersion::V5_20,
    );
    assert!(
        alias_output.diagnostics.is_empty(),
        "{:?}",
        alias_output.diagnostics
    );
}

#[test]
fn v520_nominal_identity_survives_aliases_and_separates_modules() {
    let accepted = check_output_with_version(
        &[
            (
                "main",
                "import alpha { User as AlphaUser }\nimport beta { User as BetaUser }\nfunction alphaName(value: AlphaUser) -> string { value.name }\nfunction betaName(value: BetaUser) -> string { value.name }\nlet a: AlphaUser = AlphaUser { name: \"Ada\" }\nlet b: BetaUser = BetaUser { name: \"Bea\" }\nprint(alphaName(a))\nprint(betaName(b))\n",
            ),
            ("alpha", "export record User { name: string }"),
            ("beta", "export record User { name: string }"),
        ],
        LangVersion::V5_20,
    );
    assert!(
        accepted.diagnostics.is_empty(),
        "{:?}",
        accepted.diagnostics
    );

    let rejected = check_output_with_version(
        &[
            (
                "main",
                "import alpha { User as AlphaUser }\nimport beta { User as BetaUser }\nfunction takeAlpha(value: AlphaUser) -> () { () }\nlet b: BetaUser = BetaUser { name: \"Bea\" }\ntakeAlpha(b)\n",
            ),
            ("alpha", "export record User { name: string }"),
            ("beta", "export record User { name: string }"),
        ],
        LangVersion::V5_20,
    );
    assert!(
        rejected
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "TPZ5001"),
        "expected cross-module nominal mismatch, got: {:?}",
        rejected.diagnostics
    );
}
