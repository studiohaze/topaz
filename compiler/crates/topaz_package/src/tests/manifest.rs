use super::*;
use crate::manifest::parse::validate_lispex_application_binding_parts;
use crate::*;

#[test]
fn parses_bounded_lispex_manifest_with_unicode_rule_names() {
    for name in ["환불규칙", "правилоВозврата"] {
        let manifest = parse_manifest(&lispex_manifest(name)).expect("bounded Lispex manifest");
        let lispex = manifest.lispex.expect("Lispex declaration");
        assert_eq!(lispex.profile, LISPEX_BOUNDED_PROFILE_ID);
        assert_eq!(lispex.application, None);
        assert_eq!(lispex.application_quotas, None);
        assert_eq!(lispex.rules[0].name, name);
        assert_eq!(lispex.rules[0].source, "rules/refund.lspx");
    }
}

#[test]
fn application_profile_is_exact_future_bound_and_reserves_api_exports() {
    let error = parse_manifest(&lispex_application_manifest("환불규칙"))
        .expect_err("5.17 application profile must remain unavailable");
    assert!(
        error
            .message()
            .contains("requires [package].language `5.18`")
    );

    let lispex = LispexConfig {
        profile: LISPEX_BOUNDED_PROFILE_ID.to_string(),
        application: Some(LISPEX_APPLICATION_PROFILE_ID.to_string()),
        application_quotas: Some("rules/application.quotas.json".to_string()),
        rules: Vec::new(),
    };
    let exact_std = BTreeMap::from([(
        "std".to_string(),
        Dependency {
            version: Some(LISPEX_APPLICATION_STD_VERSION.to_string()),
            path: None,
            hash: None,
        },
    )]);
    validate_lispex_application_binding_parts(
        LISPEX_APPLICATION_LANGUAGE,
        &exact_std,
        Some(&lispex),
    )
    .expect("exact dormant 5.18 application binding");
    assert!(
        validate_lispex_application_binding_parts(LangVersion::V5_17, &exact_std, Some(&lispex),)
            .is_err()
    );
    let crossed_std = BTreeMap::from([(
        "std".to_string(),
        Dependency {
            version: Some("5.17".to_string()),
            path: None,
            hash: None,
        },
    )]);
    assert!(
        validate_lispex_application_binding_parts(
            LISPEX_APPLICATION_LANGUAGE,
            &crossed_std,
            Some(&lispex),
        )
        .is_err()
    );

    let unsupported = lispex_application_manifest("환불규칙").replace(
        LISPEX_APPLICATION_PROFILE_ID,
        "topaz/lispex-decision-application/latest",
    );
    assert!(parse_manifest(&unsupported).is_err());
    let collision = lispex_application_manifest("evaluate");
    let error = parse_manifest(&collision).expect_err("reserved API export");
    assert!(error.message().contains("reserved std.lispex export"));
}

#[test]
fn complete_application_binding_is_exact_and_selectable_in_v519() {
    let lispex = LispexConfig {
        profile: LISPEX_COMPLETE_PROFILE_ID.to_string(),
        application: Some(LISPEX_COMPLETE_APPLICATION_PROFILE_ID.to_string()),
        application_quotas: Some("rules/application.quotas.json".to_string()),
        rules: Vec::new(),
    };
    let exact_std = BTreeMap::from([(
        "std".to_string(),
        Dependency {
            version: Some(LISPEX_COMPLETE_APPLICATION_STD_VERSION.to_string()),
            path: None,
            hash: None,
        },
    )]);
    validate_lispex_application_binding_parts(
        LISPEX_COMPLETE_APPLICATION_LANGUAGE,
        &exact_std,
        Some(&lispex),
    )
    .expect("exact 5.20 complete application binding");

    let bounded_identity = LispexConfig {
        profile: LISPEX_BOUNDED_PROFILE_ID.to_string(),
        ..lispex.clone()
    };
    assert!(
        validate_lispex_application_binding_parts(
            LISPEX_COMPLETE_APPLICATION_LANGUAGE,
            &exact_std,
            Some(&bounded_identity),
        )
        .is_err()
    );
    assert!(
        validate_lispex_application_binding_parts(LangVersion::V5_18, &exact_std, Some(&lispex),)
            .is_err()
    );

    let current_manifest = lispex_application_manifest("refund")
        .replace("language = \"5.4\"", "language = \"5.20\"")
        .replace("std = \"5.4\"", "std = \"5.20\"")
        .replace(LISPEX_BOUNDED_PROFILE_ID, LISPEX_COMPLETE_PROFILE_ID)
        .replace(
            LISPEX_APPLICATION_PROFILE_ID,
            LISPEX_COMPLETE_APPLICATION_PROFILE_ID,
        );
    let manifest = parse_manifest(&current_manifest).expect("5.20 complete application");
    assert_eq!(manifest.package.language, LangVersion::V5_20);
    assert_eq!(manifest.dependencies.get("std"), exact_std.get("std"));
    let parsed_lispex = manifest.lispex.expect("complete Lispex application");
    assert_eq!(parsed_lispex.profile, LISPEX_COMPLETE_PROFILE_ID);
    assert_eq!(
        parsed_lispex.application.as_deref(),
        Some(LISPEX_COMPLETE_APPLICATION_PROFILE_ID)
    );
    assert_eq!(
        parsed_lispex.application_quotas.as_deref(),
        Some("rules/application.quotas.json")
    );
    assert_eq!(parsed_lispex.rules.len(), 1);
    assert_eq!(parsed_lispex.rules[0].name, "refund");
}

#[test]
fn rejects_noncanonical_lispex_declarations() {
    let full_profile = lispex_manifest("refund").replace(
        LISPEX_BOUNDED_PROFILE_ID,
        "lispex/r7rs-rule-current-profile-bounded/1",
    );
    assert!(parse_manifest(&full_profile).is_err());

    let duplicate = format!(
        "{}\n[[lispex.rule]]\nname = \"refund\"\nsource = \"rules/other.lspx\"\nlimits = \"rules/other.json\"\n",
        lispex_manifest("refund")
    );
    assert!(parse_manifest(&duplicate).is_err());

    let escape = lispex_manifest("refund").replace("rules/refund.lspx", "../private/refund.lspx");
    assert!(parse_manifest(&escape).is_err());

    let unknown = lispex_manifest("refund").replace(
        "limits = \"rules/refund.limits.json\"",
        "limits = \"rules/refund.limits.json\"\nfallback = true",
    );
    assert!(parse_manifest(&unknown).is_err());
}

#[test]
fn parses_v54_manifest_shape() {
    let manifest = parse_manifest(&manifest_text()).expect("manifest parses");
    assert_eq!(manifest.package.name, "user_tools");
    assert_eq!(manifest.package.language, LangVersion::V5_4);
    assert_eq!(manifest.package.entry, "src/main.tpz");
    assert_eq!(
        manifest.dependencies["csv_tools"].version.as_deref(),
        Some("1.2.0")
    );
    assert_eq!(
        manifest.dependencies["local_schema"].path.as_deref(),
        Some("../schema")
    );
    assert_eq!(manifest.capabilities.fs.read, ["data", "templates"]);
    assert!(manifest.externs.is_empty());
    assert_eq!(
        manifest.exports.as_ref().map(|e| e.module.as_str()),
        Some("src/lib.tpz")
    );
}

#[test]
fn manifest_rejects_unknown_fields_and_ambiguous_dependency_sources() {
    let cases = [
        (
            format!("{}\n[deployment]\nchannel = \"stable\"\n", manifest_text()),
            "topaz.toml: unknown key `deployment`",
        ),
        (
            manifest_text().replace(
                "license = \"Apache-2.0\"",
                "license = \"Apache-2.0\"\nrepository = \"https://example.invalid\"",
            ),
            "[package]: unknown key `repository`",
        ),
        (
            manifest_text().replace(
                "deterministic = true",
                "deterministic = true\nartifact = \"app\"",
            ),
            "[build]: unknown key `artifact`",
        ),
        (
            manifest_text().replace(
                "csv_tools = { version = \"1.2.0\" }",
                "csv_tools = { version = \"1.2.0\", mirror = \"local\" }",
            ),
            "[dependencies].csv_tools: unknown key `mirror`",
        ),
        (
            manifest_text().replace(
                "module = \"src/lib.tpz\"",
                "module = \"src/lib.tpz\"\nalias = \"lib\"",
            ),
            "[exports]: unknown key `alias`",
        ),
        (
            manifest_text().replace(
                "csv_tools = { version = \"1.2.0\" }",
                &format!(
                    "csv_tools = {{ version = \"1.2.0\", path = \"../csv\", hash = \"{HASH}\" }}"
                ),
            ),
            "[dependencies].csv_tools must include exactly one of `version` or `path`",
        ),
    ];
    for (text, expected) in cases {
        let error = parse_manifest(&text).expect_err("invalid manifest must reject");
        assert!(error.message().contains(expected), "{error}");
    }
}

#[test]
fn parses_v55_manifest_language() {
    let manifest_text = manifest_text()
        .replace("language = \"5.4\"", "language = \"5.5\"")
        .replace("std = \"5.4\"", "std = \"5.5\"");
    let manifest = parse_manifest(&manifest_text).expect("manifest parses");
    assert_eq!(manifest.package.language, LangVersion::V5_5);
    assert_eq!(manifest.dependencies["std"].version.as_deref(), Some("5.5"));
}

#[test]
fn parses_v56_manifest_language_as_a_distinct_mode() {
    let manifest_text = manifest_text()
        .replace("language = \"5.4\"", "language = \"5.6\"")
        .replace("std = \"5.4\"", "std = \"5.6\"");
    let manifest = parse_manifest(&manifest_text).expect("manifest parses");
    assert_eq!(manifest.package.language, LangVersion::V5_6);
    assert_eq!(manifest.dependencies["std"].version.as_deref(), Some("5.6"));
}

#[test]
fn accepts_compatible_v57_manifest_language() {
    let manifest_text = manifest_text()
        .replace("language = \"5.4\"", "language = \"5.7\"")
        .replace("std = \"5.4\"", "std = \"5.7\"");
    let manifest = parse_manifest(&manifest_text).expect("5.7 remains compatible");
    assert_eq!(manifest.package.language, LangVersion::V5_7);
    assert_eq!(manifest.dependencies["std"].version.as_deref(), Some("5.7"));
}

#[test]
fn accepts_compatible_v58_manifest_language() {
    let manifest_text = manifest_text()
        .replace("language = \"5.4\"", "language = \"5.8\"")
        .replace("std = \"5.4\"", "std = \"5.8\"");
    let manifest = parse_manifest(&manifest_text).expect("5.8 remains compatible");
    assert_eq!(manifest.package.language, LangVersion::V5_8);
    assert_eq!(manifest.dependencies["std"].version.as_deref(), Some("5.8"));
}

#[test]
fn accepts_compatible_v59_manifest_language() {
    let manifest_text = manifest_text()
        .replace("language = \"5.4\"", "language = \"5.9\"")
        .replace("std = \"5.4\"", "std = \"5.9\"");
    let manifest = parse_manifest(&manifest_text).expect("5.9 remains compatible");
    assert_eq!(manifest.package.language, LangVersion::V5_9);
    assert_eq!(manifest.dependencies["std"].version.as_deref(), Some("5.9"));
}

#[test]
fn accepts_compatible_v510_manifest_language() {
    let manifest_text = manifest_text()
        .replace("language = \"5.4\"", "language = \"5.10\"")
        .replace("std = \"5.4\"", "std = \"5.10\"");
    let manifest = parse_manifest(&manifest_text).expect("5.10 remains compatible");
    assert_eq!(manifest.package.language, LangVersion::V5_10);
    assert_eq!(
        manifest.dependencies["std"].version.as_deref(),
        Some("5.10")
    );
}

#[test]
fn accepts_compatible_v511_manifest_language() {
    let manifest_text = manifest_text()
        .replace("language = \"5.4\"", "language = \"5.11\"")
        .replace("std = \"5.4\"", "std = \"5.11\"");
    let manifest = parse_manifest(&manifest_text).expect("5.11 remains compatible");
    assert_eq!(manifest.package.language, LangVersion::V5_11);
    assert_eq!(
        manifest.dependencies["std"].version.as_deref(),
        Some("5.11")
    );
}

#[test]
fn accepts_compatible_v512_manifest_language() {
    let manifest_text = manifest_text()
        .replace("language = \"5.4\"", "language = \"5.12\"")
        .replace("std = \"5.4\"", "std = \"5.12\"");
    let manifest = parse_manifest(&manifest_text).expect("5.12 remains compatible");
    assert_eq!(manifest.package.language, LangVersion::V5_12);
    assert_eq!(
        manifest.dependencies["std"].version.as_deref(),
        Some("5.12")
    );
}

#[test]
fn accepts_compatible_v513_manifest_language() {
    let manifest_text = manifest_text()
        .replace("language = \"5.4\"", "language = \"5.13\"")
        .replace("std = \"5.4\"", "std = \"5.13\"");
    let manifest = parse_manifest(&manifest_text).expect("5.13 remains compatible");
    assert_eq!(manifest.package.language, LangVersion::V5_13);
    assert_eq!(
        manifest.dependencies["std"].version.as_deref(),
        Some("5.13")
    );
}

#[test]
fn accepts_compatible_v517_manifest_language() {
    let manifest_text = manifest_text()
        .replace("language = \"5.4\"", "language = \"5.17\"")
        .replace("std = \"5.4\"", "std = \"5.17\"");
    let manifest = parse_manifest(&manifest_text).expect("5.17 remains compatible");
    assert_eq!(manifest.package.language, LangVersion::V5_17);
    assert_eq!(
        manifest.dependencies["std"].version.as_deref(),
        Some("5.17")
    );
}

#[test]
fn accepts_current_v518_manifest_language() {
    let manifest_text = manifest_text()
        .replace("language = \"5.4\"", "language = \"5.18\"")
        .replace("std = \"5.4\"", "std = \"5.18\"");
    let manifest = parse_manifest(&manifest_text).expect("5.18 is current");
    assert_eq!(manifest.package.language, LangVersion::V5_18);
    assert_eq!(
        manifest.dependencies["std"].version.as_deref(),
        Some("5.18")
    );
}

#[test]
fn accepts_current_v520_manifest_language() {
    let manifest_text = manifest_text()
        .replace("language = \"5.4\"", "language = \"5.20\"")
        .replace("std = \"5.4\"", "std = \"5.20\"");
    let manifest = parse_manifest(&manifest_text).expect("5.20 is current");
    assert_eq!(manifest.package.language, LangVersion::V5_20);
    assert_eq!(
        manifest.dependencies["std"].version.as_deref(),
        Some("5.20")
    );
}

#[test]
fn parses_extern_manifest_shape() {
    let text = format!(
        r#"[package]
name = "extern_demo"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.host.image]
hash = "{HASH}"
abi_hash = "{HASH}"

[[extern.host.image.functions]]
name = "resizePng"
params = ["Bytes", " int ", "Option<Result<Bytes, string>>"]
result = "Result<Bytes, string>"

[extern.host.image.replay]
fixture = "./fixtures/host-image-replay.json"

[extern.host.image.sandbox]
kind = "replay"
"#
    );
    let manifest = parse_manifest(&text).expect("manifest parses");
    let module = &manifest.externs["host.image"];
    assert_eq!(module.hash, HASH);
    assert_eq!(module.abi_hash, HASH);
    assert_eq!(module.sandbox.kind, ExternSandboxKind::Replay);
    assert_eq!(module.sandbox.fuel, None);
    assert_eq!(module.sandbox.memory_bytes, None);
    assert_eq!(module.replay.fixture, "fixtures/host-image-replay.json");
    let function = &module.functions[0];
    assert_eq!(function.name, "resizePng");
    assert_eq!(
        function
            .params
            .iter()
            .map(AbiType::canonical)
            .collect::<Vec<_>>(),
        ["Bytes", "int", "Option<Result<Bytes,string>>"]
    );
    assert_eq!(function.result.canonical(), "Result<Bytes,string>");
}

#[test]
fn parses_public_extern_abi_type_canonical_forms() {
    let ty = parse_abi_type(" ( ) ").expect("unit parses");
    assert_eq!(ty, AbiType::Unit);
    assert_eq!(ty.canonical(), "()");

    let array = parse_abi_type(" Array < Option < int > > ").expect("array parses");
    assert_eq!(array.canonical(), "Array<Option<int>>");

    let option = parse_abi_type("Option<Result<Bytes, string>>").expect("option parses");
    assert_eq!(option.canonical(), "Option<Result<Bytes,string>>");

    let err = parse_abi_type("Array<int> trailing").unwrap_err();
    assert!(err.message().contains("trailing text"), "{err}");

    let err = parse_abi_type("Result<Bytes").unwrap_err();
    assert!(err.message().contains("malformed extern ABI type"), "{err}");
}

#[test]
fn extern_manifest_rejects_empty_extern_table() {
    let text = r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern]
"#;
    let err = parse_manifest(text).unwrap_err();
    assert!(
        err.message()
            .contains("[extern] must contain extern module tables"),
        "{err}"
    );
}

#[test]
fn extern_manifest_rejects_bad_hash() {
    let text = format!(
        r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.host.image]
hash = "sha256:bad"
abi_hash = "{HASH}"

[[extern.host.image.functions]]
name = "resize"
params = ["Bytes"]
result = "Bytes"

[extern.host.image.replay]
fixture = "fixtures/replay.json"
"#
    );
    let err = parse_manifest(&text).unwrap_err();
    assert!(err.message().contains("64-hex SHA-256"), "{err}");
}

#[test]
fn extern_manifest_rejects_unsupported_or_generic_abi_type() {
    let text = format!(
        r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.host.image]
hash = "{HASH}"
abi_hash = "{HASH}"

[[extern.host.image.functions]]
name = "resize"
params = ["Array<T>"]
result = "File"

[extern.host.image.replay]
fixture = "fixtures/replay.json"
"#
    );
    let err = parse_manifest(&text).unwrap_err();
    assert!(
        err.message().contains("unsupported extern ABI type `T`"),
        "{err}"
    );
}

#[test]
fn extern_manifest_rejects_missing_or_unknown_replay() {
    let missing = format!(
        r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.host.image]
hash = "{HASH}"
abi_hash = "{HASH}"

[[extern.host.image.functions]]
name = "resize"
params = ["Bytes"]
result = "Bytes"
"#
    );
    let err = parse_manifest(&missing).unwrap_err();
    assert!(err.message().contains("missing `[replay]`"), "{err}");

    let unknown = format!(
        r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.host.image]
hash = "{HASH}"
abi_hash = "{HASH}"

[[extern.host.image.functions]]
name = "resize"
params = ["Bytes"]
result = "Bytes"

[extern.host.image.replay]
fixture = "fixtures/replay.json"
mode = "live"
"#
    );
    let err = parse_manifest(&unknown).unwrap_err();
    assert!(err.message().contains("unknown key `mode`"), "{err}");
}

#[test]
fn extern_manifest_rejects_bad_sandbox_policy() {
    let unsupported_kind = format!(
        r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.host.image]
hash = "{HASH}"
abi_hash = "{HASH}"

[[extern.host.image.functions]]
name = "resize"
params = ["Bytes"]
result = "Bytes"

[extern.host.image.replay]
fixture = "fixtures/replay.json"

[extern.host.image.sandbox]
kind = "native"
"#
    );
    let err = parse_manifest(&unsupported_kind).unwrap_err();
    assert!(
        err.message().contains("must be `replay` or `wasm`"),
        "{err}"
    );

    let missing_artifact = format!(
        r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.host.image]
hash = "{HASH}"
abi_hash = "{HASH}"

[[extern.host.image.functions]]
name = "resize"
params = ["Bytes"]
result = "Bytes"

[extern.host.image.replay]
fixture = "fixtures/replay.json"

[extern.host.image.sandbox]
kind = "wasm"
"#
    );
    let err = parse_manifest(&missing_artifact).unwrap_err();
    assert!(err.message().contains("requires `[artifact]`"), "{err}");

    let invalid_fuel = format!(
        r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.host.image]
hash = "{HASH}"
abi_hash = "{HASH}"

[[extern.host.image.functions]]
name = "resize"
params = ["Bytes"]
result = "Bytes"

[extern.host.image.replay]
fixture = "fixtures/replay.json"

[extern.host.image.sandbox]
kind = "replay"
fuel = 0
"#
    );
    let err = parse_manifest(&invalid_fuel).unwrap_err();
    assert!(err.message().contains("positive integer"), "{err}");
}

#[test]
fn extern_manifest_rejects_mixed_module_namespace() {
    let text = format!(
        r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.host.image]
hash = "{HASH}"
abi_hash = "{HASH}"

[[extern.host.image.functions]]
name = "resize"
params = ["Bytes"]
result = "Bytes"

[extern.host.image.replay]
fixture = "fixtures/replay.json"

[extern.host.image.sub]
hash = "{HASH}"
"#
    );
    let err = parse_manifest(&text).unwrap_err();
    assert!(err.message().contains("unknown key `sub`"), "{err}");
}

#[test]
fn extern_manifest_rejects_reserved_or_non_identifier_module_segments() {
    let reserved = format!(
        r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.std.image]
hash = "{HASH}"
"#
    );
    let err = parse_manifest(&reserved).unwrap_err();
    assert!(err.message().contains("std is reserved"), "{err}");

    let hyphen = format!(
        r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.host-image]
hash = "{HASH}"
"#
    );
    let err = parse_manifest(&hyphen).unwrap_err();
    assert!(err.message().contains("Topaz identifier"), "{err}");
}

#[test]
fn extern_manifest_rejects_marker_key_as_namespace_segment() {
    let text = format!(
        r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.host.replay]
hash = "{HASH}"
abi_hash = "{HASH}"

[[extern.host.replay.functions]]
name = "resize"
params = ["Bytes"]
result = "Bytes"

[extern.host.replay.replay]
fixture = "fixtures/replay.json"
"#
    );
    let err = parse_manifest(&text).unwrap_err();
    assert!(
        err.message().contains("[extern.host]: missing `hash`"),
        "{err}"
    );
}

#[test]
fn extern_manifest_rejects_empty_or_duplicate_functions() {
    let empty = format!(
        r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.host.image]
hash = "{HASH}"
abi_hash = "{HASH}"
functions = []

[extern.host.image.replay]
fixture = "fixtures/replay.json"
"#
    );
    let err = parse_manifest(&empty).unwrap_err();
    assert!(err.message().contains("must not be empty"), "{err}");

    let duplicate = format!(
        r#"[package]
name = "bad_extern"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[extern.host.image]
hash = "{HASH}"
abi_hash = "{HASH}"

[[extern.host.image.functions]]
name = "resize"
params = ["Bytes"]
result = "Bytes"

[[extern.host.image.functions]]
name = "resize"
params = []
result = "()"

[extern.host.image.replay]
fixture = "fixtures/replay.json"
"#
    );
    let err = parse_manifest(&duplicate).unwrap_err();
    assert!(err.message().contains("duplicate extern function"), "{err}");
}

#[test]
fn web_manifest_is_strict_and_normalized() {
    let manifest = parse_manifest(
        r#"[package]
name = "web_demo"
version = "0.1.0"
language = "5.6"
entry = "src/main.tpz"

[build]
target = "web-app"

[web]
title = "Web demo"
styles = ["styles/app.css"]
assets = ["assets/logo.svg"]
lifecycle = "v2"

[capabilities.web]
open_text = true
download_text = true
local_state = true
"#,
    )
    .expect("valid web manifest");
    assert_eq!(manifest.build.target, "web-app");
    assert_eq!(manifest.web.title, "Web demo");
    assert_eq!(manifest.web.styles, ["styles/app.css"]);
    assert_eq!(manifest.web.assets, ["assets/logo.svg"]);
    assert_eq!(manifest.web.lifecycle, WebLifecycle::V2);
    assert!(manifest.capabilities.web.open_text);
    assert!(manifest.capabilities.web.download_text);
    assert!(manifest.capabilities.web.local_state);

    for (needle, extra) in [
        ("unknown key `script`", "script = \"app.js\""),
        ("must be a `.css` file", "styles = [\"assets/app.css\"]"),
        ("must be under `assets/`", "assets = [\"images/logo.svg\"]"),
        (
            "must not contain a query, fragment, or control character",
            "assets = [\"assets/logo.svg?v=1\"]",
        ),
    ] {
        let text = format!(
            "[package]\nname = \"web_demo\"\nversion = \"0.1.0\"\nlanguage = \"5.6\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"web-app\"\n\n[web]\n{extra}\n"
        );
        let error = parse_manifest(&text).expect_err("invalid web manifest");
        assert!(error.message().contains(needle), "{error}");
    }

    for (text, needle) in [
        (
            "[package]\nname = \"native_demo\"\nversion = \"0.1.0\"\nlanguage = \"5.6\"\nentry = \"src/main.tpz\"\n\n[web]\ntitle = \"wrong target\"\n",
            "[web] is allowed only",
        ),
        (
            "[package]\nname = \"bad_target\"\nversion = \"0.1.0\"\nlanguage = \"5.6\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"browser\"\n",
            "[build].target `browser` is unsupported",
        ),
        (
            "[package]\nname = \"bad_lifecycle\"\nversion = \"0.1.0\"\nlanguage = \"5.6\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"web-app\"\n\n[web]\nlifecycle = \"v3\"\n",
            "[web].lifecycle `v3` is unsupported",
        ),
        (
            "[package]\nname = \"native_capability\"\nversion = \"0.1.0\"\nlanguage = \"5.6\"\nentry = \"src/main.tpz\"\n\n[capabilities.web]\nopen_text = true\n",
            "[capabilities.web] is allowed only",
        ),
        (
            "[package]\nname = \"native_empty_web_capability\"\nversion = \"0.1.0\"\nlanguage = \"5.6\"\nentry = \"src/main.tpz\"\n\n[capabilities.web]\n",
            "[capabilities.web] is allowed only",
        ),
        (
            "[package]\nname = \"unknown_capability\"\nversion = \"0.1.0\"\nlanguage = \"5.6\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"web-app\"\n\n[capabilities.web]\nclipboard = true\n",
            "unknown key `clipboard`",
        ),
        (
            "[package]\nname = \"unknown_fs_capability\"\nversion = \"0.1.0\"\nlanguage = \"5.6\"\nentry = \"src/main.tpz\"\n\n[capabilities.fs]\nexecute = [\"bin\"]\n",
            "unknown key `execute`",
        ),
    ] {
        let error = parse_manifest(text).expect_err("invalid target boundary");
        assert!(error.message().contains(needle), "{error}");
    }
}

#[test]
fn http_service_manifest_is_strict_bounded_and_target_scoped() {
    let manifest = parse_manifest(
        r#"[package]
name = "service_demo"
version = "0.1.0"
language = "5.9"
entry = "src/main.tpz"

[build]
target = "http-service"

[service]
bind = "::1"
port = 9000
workers = 4
max_connections = 128
queue_capacity = 0
max_target_bytes = 4096
max_header_bytes = 8192
max_headers = 32
max_body_bytes = 0
header_timeout_ms = 1000
body_timeout_ms = 2000
handler_timeout_ms = 500
shutdown_grace_ms = 0
log_format = "json"
"#,
    )
    .expect("valid service manifest");
    assert_eq!(manifest.build.target, "http-service");
    assert_eq!(manifest.service.bind, "::1");
    assert_eq!(manifest.service.port, 9000);
    assert_eq!(manifest.service.workers, 4);
    assert_eq!(manifest.service.queue_capacity, 0);
    assert_eq!(manifest.service.max_body_bytes, 0);
    assert_eq!(manifest.service.log_format, ServiceLogFormat::Json);

    let defaults = parse_manifest(
        "[package]\nname = \"service_defaults\"\nversion = \"0.1.0\"\nlanguage = \"5.9\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"http-service\"\n",
    )
    .expect("service defaults");
    assert_eq!(defaults.service, ServiceConfig::default());

    for (extra, needle) in [
        ("bind = \"0.0.0.0\"", "must be a loopback IP literal"),
        ("port = 0", "must be in 1..=65535"),
        ("workers = 65", "must be in 1..=64"),
        ("max_body_bytes = 16777217", "must be in 0..=16777216"),
        ("handler_timeout_ms = \"fast\"", "must be an integer"),
        ("log_format = \"verbose\"", "is unsupported"),
        ("router = \"magic\"", "unknown key `router`"),
    ] {
        let text = format!(
            "[package]\nname = \"bad_service\"\nversion = \"0.1.0\"\nlanguage = \"5.9\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"http-service\"\n\n[service]\n{extra}\n"
        );
        let error = parse_manifest(&text).expect_err("invalid service config");
        assert!(error.message().contains(needle), "{error}");
    }

    let error = parse_manifest(
        "[package]\nname = \"native_service_config\"\nversion = \"0.1.0\"\nlanguage = \"5.9\"\nentry = \"src/main.tpz\"\n\n[service]\nport = 9000\n",
    )
    .expect_err("service section is target scoped");
    assert!(error.message().contains("[service] is allowed only"));
}
