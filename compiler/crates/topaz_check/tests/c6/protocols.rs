use super::*;

#[test]
fn generated_lispex_contract_is_opaque_and_records_direct_rule_reachability() {
    const API: &str = include_str!("../../../../contracts/lispex-application/v1/std.lispex.tpz");
    const RULES: &str = "import std.lispex { PreparedLispexRule }\n\nexport function 환불() -> PreparedLispexRule {\n    __lispexRule(\"환불\")\n}\n";
    const MAIN: &str = "import std.lispex\nimport std.lispex.rules as rules\nlet rule = rules.환불()\nlet limits = lispex.defaultLimits(rule)\n";
    let sources = [API, RULES, MAIN];
    let identities = ["std.lispex", "std.lispex.rules", "main"];
    let programs = sources
        .iter()
        .enumerate()
        .map(|(index, source)| parse_at(index as u32, source, LangVersion::V5_18))
        .collect::<Vec<_>>();
    let modules = sources
        .iter()
        .zip(programs.iter())
        .enumerate()
        .map(|(index, (source, program))| UnitModule {
            identity: identities[index].to_string(),
            is_entry: index == 2,
            is_extern: false,
            is_generated_std: index < 2,
            extern_replay_error: None,
            src: source,
            program,
        })
        .collect::<Vec<_>>();
    let checked = check_unit_typed_with_version(&modules, LangVersion::V5_18);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let targets = checked
        .typed_hir
        .expect("clean typed HIR")
        .calls
        .into_iter()
        .filter_map(|call| call.target_identity)
        .collect::<Vec<_>>();
    assert_eq!(targets, ["topaz.lispex-rule-handle/v1:환불"]);

    let indirect_source =
        "import std.lispex.rules as rules\nlet factory = rules.환불\nlet rule = factory()\n";
    let indirect_program = parse_at(3, indirect_source, LangVersion::V5_18);
    let indirect = UnitModule {
        identity: "indirect".to_string(),
        is_entry: true,
        is_extern: false,
        is_generated_std: false,
        extern_replay_error: None,
        src: indirect_source,
        program: &indirect_program,
    };
    let mut indirect_modules = modules[..2]
        .iter()
        .map(|module| UnitModule {
            identity: module.identity.clone(),
            is_entry: false,
            is_extern: module.is_extern,
            is_generated_std: module.is_generated_std,
            extern_replay_error: module.extern_replay_error.clone(),
            src: module.src,
            program: module.program,
        })
        .collect::<Vec<_>>();
    indirect_modules.push(indirect);
    let rejected = check_unit_with_version(&indirect_modules, LangVersion::V5_18);
    assert!(rejected.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("may only appear as the direct callee")
    }));

    let forged_source = "import std.lispex\nenum __PreparedLispexRuleCarrier { __PreparedLispexRuleValue }\nlet fake = __PreparedLispexRuleCarrier.__PreparedLispexRuleValue\nlet limits = lispex.defaultLimits(fake)\n";
    let forged_program = parse_at(4, forged_source, LangVersion::V5_18);
    let forged = UnitModule {
        identity: "forged".to_string(),
        is_entry: true,
        is_extern: false,
        is_generated_std: false,
        extern_replay_error: None,
        src: forged_source,
        program: &forged_program,
    };
    let api = UnitModule {
        identity: modules[0].identity.clone(),
        is_entry: false,
        is_extern: false,
        is_generated_std: true,
        extern_replay_error: None,
        src: modules[0].src,
        program: modules[0].program,
    };
    let forged_output = check_unit_with_version(&[api, forged], LangVersion::V5_18);
    assert!(
        forged_output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "TPZ5001"),
        "{:?}",
        forged_output.diagnostics
    );

    let hidden_import_source = "import std.lispex { __PreparedLispexRuleCarrier }\nlet fake = __PreparedLispexRuleCarrier.__PreparedLispexRuleValue\n";
    let hidden_import_program = parse_at(5, hidden_import_source, LangVersion::V5_18);
    let hidden_import = UnitModule {
        identity: "hidden-import".to_string(),
        is_entry: true,
        is_extern: false,
        is_generated_std: false,
        extern_replay_error: None,
        src: hidden_import_source,
        program: &hidden_import_program,
    };
    let api = UnitModule {
        identity: modules[0].identity.clone(),
        is_entry: false,
        is_extern: false,
        is_generated_std: true,
        extern_replay_error: None,
        src: modules[0].src,
        program: modules[0].program,
    };
    let hidden_import_output = check_unit_with_version(&[api, hidden_import], LangVersion::V5_18);
    assert!(
        hidden_import_output.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("`__PreparedLispexRuleCarrier` is not exported by `std.lispex`")
        }),
        "{:?}",
        hidden_import_output.diagnostics
    );

    let intrinsic_source = "let fake = __lispexRule(\"refund\")\n";
    let intrinsic_program = parse_at(6, intrinsic_source, LangVersion::V5_18);
    let intrinsic = UnitModule {
        identity: "intrinsic".to_string(),
        is_entry: true,
        is_extern: false,
        is_generated_std: false,
        extern_replay_error: None,
        src: intrinsic_source,
        program: &intrinsic_program,
    };
    let intrinsic_output = check_unit_with_version(&[intrinsic], LangVersion::V5_18);
    assert!(
        intrinsic_output
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("`__lispexRule` is not bound") }),
        "{:?}",
        intrinsic_output.diagnostics
    );
}

#[test]
fn explicit_main_signature_is_entry_only() {
    assert_message_contains(
        &[("main", "export function main() -> int {\n  return 0\n}\n")],
        "must be non-generic and have signature",
    );
    assert_clean(&[
        ("main", "import helper\nlet x = helper.main()\n"),
        ("helper", "export function main() -> int {\n  return 1\n}\n"),
    ]);
}

// ---- v5.4 unit-level protocol conformance table ----------------------------

#[test]
fn unit_check_output_keeps_derived_conformance_table() {
    let out = check_output_with_version(
        &[("main", "record P derives Eq, Show { a: int }\n0")],
        LangVersion::V5_4,
    );
    assert!(
        out.diagnostics.is_empty(),
        "expected clean, got: {:?}",
        out.diagnostics
    );
    assert_eq!(
        out.conformances,
        vec![
            ("Eq".to_string(), "P".to_string()),
            ("Show".to_string(), "P".to_string()),
        ],
    );
}

#[test]
fn imported_derived_conformance_is_visible_to_protocol_call() {
    let out = check_output_with_version(
        &[
            (
                "model",
                "record User derives Show { name: string }\nexport function make() -> User {\n    return User { name: \"Ada\" }\n}\n0",
            ),
            (
                "main",
                "import model { make }\nlet s: string = Show.show(make())\nprint(s)",
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        out.diagnostics.is_empty(),
        "expected clean, got: {:?}",
        out.diagnostics
    );
    assert_eq!(
        out.conformances,
        vec![("Show".to_string(), "User".to_string())],
    );
}

#[test]
fn local_protocol_signatures_resolve_selected_imported_nominals() {
    let out = check_output_with_version(
        &[
            ("model", "export record Token { value: int }\n0"),
            (
                "main",
                "import model { Token }\nprotocol Transform { function apply(value: Self, token: Token) -> Token }\nrecord Handler {}\nimpl Transform<Handler> { function apply(value: Handler, token: Token) -> Token { token } }\nlet token = Transform.apply(Handler {}, Token { value: 1 })\nprint(\"{token.value}\")",
            ),
        ],
        LangVersion::V5_4,
    );
    assert!(
        out.diagnostics.is_empty(),
        "expected clean imported protocol signature, got: {:?}",
        out.diagnostics
    );
}

#[test]
fn manual_and_user_protocol_conformances_do_not_leak_by_name_across_modules() {
    assert_code_at(
        &[
            (
                "main",
                "import model { make }\nlet s: string = Show.show(make())\nprint(s)",
            ),
            (
                "model",
                "record User { name: string }\nimpl Show<User> { function show(value: User) -> string { value.name } }\nexport function make() -> User { User { name: \"Ada\" } }",
            ),
        ],
        LangVersion::V5_4,
        "TPZ5522",
    );

    assert_code_at(
        &[
            (
                "main",
                "import model { make }\nprotocol Label { function label(value: Self) -> int }\nlet n: int = Label.label(make())\nprint(\"{n}\")",
            ),
            (
                "model",
                "protocol Label { function label(value: Self) -> string }\nrecord User { name: string }\nimpl Label<User> { function label(value: User) -> string { value.name } }\nexport function make() -> User { User { name: \"Ada\" } }",
            ),
        ],
        LangVersion::V5_4,
        "TPZ5522",
    );
}
