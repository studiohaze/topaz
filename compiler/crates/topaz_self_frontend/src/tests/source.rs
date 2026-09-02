use super::*;

#[test]
fn embedded_exchange_is_pure_and_versioned() {
    let request = br#"{"schema":"topaz.compiler.frontend-preview-exchange/v1","terminal":"ast","entry":"fixture","root":"","source":"let answer = 42\n","sourceId":"fixture","facts":[],"package":{"buildRole":"standalone","externModules":[],"externReplayModules":[],"externReplayErrors":[],"generatedStdModules":[]},"maxAstNodes":2000000,"maxAstDepth":1024}"#;
    let response = invoke_exchange(request).expect("exchange");
    let text = std::str::from_utf8(&response).expect("UTF-8 response");
    assert!(text.contains(EXCHANGE_SCHEMA));
    assert!(text.contains("\"kind\":\"keyword/let\""));
    assert!(text.contains("\"kind\":\"integer\""));
    assert!(text.contains("\"stream\":\"raw\""));
    assert!(text.contains("\"stream\":\"layout\""));
}

#[test]
fn source_identity_is_ordered_and_content_addressed() {
    assert!(SOURCES.windows(2).all(|pair| pair[0].path < pair[1].path));
    let identity = source_set_id();
    assert!(identity.starts_with("sha256:"));
    assert_eq!(identity.len(), 71);
    let manifest = source_manifest();
    assert_eq!(
        manifest.schema,
        "topaz.compiler.embedded-source-manifest/v1"
    );
    assert_eq!(manifest.source_set_id, identity);
    assert_eq!(manifest.files.len(), SOURCES.len());
    assert!(
        manifest
            .files
            .iter()
            .all(|file| file.content_sha256.len() == 71)
    );
}

#[test]
fn target_source_identity_tracks_order_content_and_entry_ownership() {
    let module = |identity: &str, source: &str, entry: bool| topaz_kernel::CanonicalPreviewModule {
        identity: identity.to_string(),
        path: format!("{identity}.tpz"),
        source: source.to_string(),
        entry,
        extern_module: false,
        generated_std: false,
        raw: Vec::new(),
        layout: Vec::new(),
        ast: Vec::new(),
    };
    let main = module("main", "let answer = 42\n", true);
    let util = module("util", "export const value = 7\n", false);
    let identity = target_source_set_id(&[main.clone(), util.clone()]).expect("target identity");
    assert_ne!(
        identity,
        target_source_set_id(&[util.clone(), main.clone()]).expect("ordered target identity")
    );
    assert_ne!(
        identity,
        target_source_set_id(&[module("main", "let answer = 43\n", true), util.clone(),])
            .expect("content-addressed target identity")
    );
    assert!(target_source_set_id(&[main.clone(), main]).is_err());
    assert!(target_source_set_id(&[util]).is_err());
}

#[test]
fn unsupported_exchange_never_runs_a_target_fallback() {
    let request =
            br#"{"schema":"topaz.compiler.frontend-preview-exchange/v0","terminal":"ast","entry":"fixture","root":"","source":"@","sourceId":"fixture","facts":[],"package":{"buildRole":"standalone","externModules":[],"externReplayModules":[],"externReplayErrors":[],"generatedStdModules":[]},"maxAstNodes":2000000,"maxAstDepth":1024}"#;
    let error = invoke_exchange(request).expect_err("schema must decline");
    assert!(error.contains("unsupported front-end preview exchange schema"));
    assert!(!error.contains("unexpected character"));
}

#[test]
fn unsupported_exchange_terminal_stops_before_source_processing() {
    let request =
            br#"{"schema":"topaz.compiler.frontend-preview-exchange/v1","terminal":"unknown","entry":"fixture","root":"","source":"@","sourceId":"fixture","facts":[],"package":{"buildRole":"standalone","externModules":[],"externReplayModules":[],"externReplayErrors":[],"generatedStdModules":[]},"maxAstNodes":2000000,"maxAstDepth":1024}"#;
    let error = invoke_exchange(request).expect_err("terminal must decline");
    assert!(
        error.contains("unsupported front-end preview terminal `unknown`"),
        "{error}"
    );
    assert!(!error.contains("unexpected character"));
}

#[test]
fn ast_preview_enforces_private_node_and_depth_limits() {
    let node_limit = br#"{"schema":"topaz.compiler.frontend-preview-exchange/v1","terminal":"ast","entry":"fixture","root":"","source":"let answer = 42\n","sourceId":"fixture","facts":[],"package":{"buildRole":"standalone","externModules":[],"externReplayModules":[],"externReplayErrors":[],"generatedStdModules":[]},"maxAstNodes":1,"maxAstDepth":1024}"#;
    let error = invoke_exchange(node_limit).expect_err("node limit must stop");
    assert!(
        error.contains("front-end preview AST node count exceeds 1"),
        "{error}"
    );

    let depth_limit = br#"{"schema":"topaz.compiler.frontend-preview-exchange/v1","terminal":"ast","entry":"fixture","root":"","source":"let answer = 42\n","sourceId":"fixture","facts":[],"package":{"buildRole":"standalone","externModules":[],"externReplayModules":[],"externReplayErrors":[],"generatedStdModules":[]},"maxAstNodes":2000000,"maxAstDepth":0}"#;
    let error = invoke_exchange(depth_limit).expect_err("depth limit must stop");
    assert!(
        error.contains("front-end preview AST depth exceeds 0"),
        "{error}"
    );
}

#[test]
fn token_preview_builds_a_valid_standard_observation() {
    let source = "let answer = 42\n";
    let preview = preview_source("main.tpz", source).expect("preview source");
    let bundle = topaz_kernel::build_token_preview_observation(
        "main.tpz",
        "main",
        source,
        &preview.raw,
        &preview.layout,
        &preview.diagnostics,
    )
    .expect("token observation");
    bundle.validate().expect("valid observation");
    let provenance = bundle
        .files
        .iter()
        .find(|file| file.path == "provenance.json")
        .expect("provenance");
    let text = std::str::from_utf8(&provenance.bytes).expect("provenance UTF-8");
    assert!(text.contains("\"engine\":\"topaz-front-end-preview\""));
    assert!(text.contains("\"defaultEngine\":\"rust-stage0\""));
    assert!(text.contains("\"producerStage\":0"));
    assert!(text.contains("\"resultStage\":0"));
}

#[test]
fn ast_preview_builds_a_valid_standard_observation() {
    let source = "let answer = 42\n";
    let preview = preview_source("main.tpz", source).expect("preview source");
    let ast = vec![
        topaz_kernel::CanonicalPreviewAstNode {
            kind: "program".to_string(),
            lo: 0,
            hi: source.len() as u32,
            parent: None,
            field: "root".to_string(),
            index: 0,
            attributes: vec![],
        },
        topaz_kernel::CanonicalPreviewAstNode {
            kind: "statement/let".to_string(),
            lo: 0,
            hi: 15,
            parent: Some(0),
            field: "items".to_string(),
            index: 0,
            attributes: vec![topaz_kernel::CanonicalPreviewAstAttribute {
                name: "mutable".to_string(),
                value: topaz_kernel::CanonicalPreviewAstValue::Bool(false),
            }],
        },
    ];
    let bundle = topaz_kernel::build_ast_preview_observation(
        "main.tpz",
        "main",
        source,
        &preview.raw,
        &preview.layout,
        &ast,
        &preview.diagnostics,
    )
    .expect("AST observation");
    bundle.validate().expect("valid AST observation");
    let response = bundle
        .files
        .iter()
        .find(|file| file.path == "response.json")
        .expect("response");
    let text = std::str::from_utf8(&response.bytes).expect("response UTF-8");
    assert!(text.contains("\"highestCompletedPhase\":\"ast\""));
    assert!(text.contains("\"ast\":\"produced\""));
    assert!(text.contains("\"resolved\":\"not-requested\""));
}

#[test]
fn parser_rejects_empty_interpolation_like_stage0() {
    let source = concat!(
        "function jsonAlias(value: JSONValue) -> () {\n",
        "  let parsed = value.parseAs<string>(\"{}\")\n",
        "}\n",
    );
    let session = FrontEndSession::new().expect("front-end session");
    let root = preview_response(&session, source);
    assert_eq!(
        preview_diagnostics(&root),
        rust_frontend_diagnostics(source)
    );
}

#[test]
fn parser_construct_braces_and_nested_record_updates_match_stage0() {
    let session = FrontEndSession::new().expect("front-end session");
    for source in [
        "function main() -> int { let ready = true\n if ready {}\n 0 }\n",
        concat!(
            "function main() -> int {\n",
            "  let value = { active: false }\n",
            "  if (value { active: true }).active { 1 } else { 0 }\n",
            "}\n",
        ),
        concat!(
            "let enabled = true\n",
            "let selected = match 1 {\n",
            "  case found if enabled => item => found + item\n",
            "  case _ => item => item\n",
            "}\n",
            "let result = selected(2)\n",
        ),
        concat!(
            "let selected = match 1 {\n",
            "  case found if ((item) => item)(true) => found\n",
            "  case _ => 0\n",
            "}\n",
        ),
        concat!(
            "let ready = true\n",
            "let selected = match 1 {\n",
            "  case found if (ready) => found\n",
            "  case _ => 0\n",
            "}\n",
        ),
        "let values = [1]\nlet result = [for value in (values) => value]\n",
        "let values = [1]\nlet result = [for value in values if (true) => value]\n",
        "if task.let {\n  one()\n  two()\n}\n",
        "let {\n  let: outer,\n  concurrent: {\n    y: Some(_)\n      | None,\n  },\n} = value\n",
        concat!(
            "let selected = match 1 {\n",
            "  case outer if match 1 { case inner => item => true } => outer\n",
            "  case _ => 0\n",
            "}\n",
        ),
    ] {
        let root = preview_response(&session, source);
        let self_diagnostics = preview_diagnostics(&root);
        assert_eq!(self_diagnostics, rust_frontend_diagnostics(source));
        assert!(
            self_diagnostics.is_empty(),
            "{source:?}: {self_diagnostics:?}"
        );
    }

    let invalid = "function make() -> int { 1 }\nlet result = make() {}\n";
    let root = preview_response(&session, invalid);
    let self_diagnostics = preview_diagnostics(&root);
    assert_eq!(self_diagnostics, rust_frontend_diagnostics(invalid));
    assert!(
        self_diagnostics
            .iter()
            .any(|(_, message, _, _)| message == "expected a statement separator"),
        "call results must not become empty nominal forms: {self_diagnostics:?}"
    );
}

#[test]
fn linked_stage2_missing_source_facts_fail_without_fallback() {
    struct MissingSourceHost;

    impl topaz_kernel::HostFactSource for MissingSourceHost {
        fn respond(
            &self,
            _request: &topaz_kernel::KernelRequest,
            query: &topaz_kernel::HostQuery,
        ) -> topaz_kernel::HostFact {
            match query {
                topaz_kernel::HostQuery::ReadSource { .. } => {
                    topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Missing)
                }
                topaz_kernel::HostQuery::ListDirectory { .. } => {
                    topaz_kernel::HostFact::Directory(topaz_kernel::DirectoryFact::Missing)
                }
                topaz_kernel::HostQuery::PhysicalContainment { logical_path, .. } => {
                    topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                        alias_class: format!("missing-stage2:{logical_path}"),
                    })
                }
            }
        }
    }

    let request = topaz_kernel::KernelRequest::checked(
        "main.tpz",
        Some(""),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::RustSource);
    let error = match preview_linked_stage2_generated(&MissingSourceHost, request) {
        Ok(_) => panic!("missing Stage 2 source facts must stop"),
        Err(error) => error,
    };
    assert!(error.contains("did not complete"), "{error}");
    assert!(!error.contains("fallback"), "{error}");
}

#[test]
fn initial_code_token_and_layout_projection_matches_stage0() {
    let session = FrontEndSession::new().expect("front-end session");
    for source in [
        "let answer = 42\nanswer += 1\n",
        "const delay = 25ms\nloop 'outer { break 'outer }\n",
        "토파즈 = 3.5 // comment\n/* block */ 토파즈 |> show\n",
        "😀🚀a٣ = 1\n",
        "(a ?? b) && [true, false].length > 0;\n",
        "let greeting = \"hello\\n\"\n",
        "let greeting = \"hello {name}!\"\n",
        "let nested = \"value {if ready { \"yes\" } else { \"no\" }}\"\n",
        "let query = sql\"\"\"select value\nfrom items\"\"\"\n",
        "let query = sql\"\"\"select {column}\nfrom {table}\"\"\"\n",
        "a;\nb\n",
        "a // comment\r\nb\r",
        "first; import pkg { item,\n other }\n",
        "value\n  |> f\n  |> g\n",
        "if ready { one() }\nelse { two() }\n",
        "match value {\n  case 1 => one\n  case _ => other\n}\n",
        "concurrent(timeout: 3s) {\n  first: f()\n  second: g()\n}\nelse { 0 }\n",
        "concurrent concurrent; { x:\n  1 }\n",
        "let point = {\n  x: 1,\n  y: 2\n}\n",
        "let { x, y } = point\n",
        "if task.let {\n  one()\n  two()\n}\n",
        "let {\n  let: outer,\n  concurrent: {\n    y: Some(_)\n      | None,\n  },\n} = value\n",
        "for { a, b } in pairs {\n  f(a)\n  g(b)\n}\n",
        "function f() -> { x: int } {\n  return { x: 1 }\n}\n",
        "f(\n  a,\n  b\n)\n",
        "\"\"\"{value\n|> render}\"\"\"\n",
    ] {
        let root = preview_response(&session, source);
        assert_eq!(preview_stream(&root, "raw"), rust_stream(source, false));
        assert_eq!(preview_stream(&root, "layout"), rust_stream(source, true));
    }
}

#[test]
fn initial_lexical_and_layout_diagnostics_match_stage0() {
    let session = FrontEndSession::new().expect("front-end session");
    for source in [
        "'",
        "let\u{000c}value = 1\n",
        "/* open",
        "\"open\n",
        "\"bad\\q\"",
        "\"bad }\"",
        "\"\"\"\n  aligned\n misaligned\n  \"\"\"",
        "\"outer {\"\"\"inner\n",
        "f(a; b)",
    ] {
        let root = preview_response(&session, source);
        assert_eq!(preview_diagnostics(&root), rust_diagnostics(source));
    }
}

#[test]
fn nested_multiline_break_recovery_tokens_match_stage0() {
    let source = "\"outer {\"\"\"inner\n";
    let session = FrontEndSession::new().expect("front-end session");
    let root = preview_response(&session, source);
    let raw = lex(FileId(0), source);
    let expected = raw
        .tokens
        .iter()
        .map(|token| {
            (
                topaz_kernel::canonical_token_kind(token.kind),
                token.span.lo,
                token.span.hi,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(preview_stream(&root, "raw"), expected);
    assert_eq!(preview_diagnostics(&root), rust_diagnostics(source));
}

#[test]
fn recovery_tokens_keep_synthetic_origin_through_layout() {
    let session = FrontEndSession::new().expect("front-end session");
    let root = preview_response(&session, "\"outer {\"\"\"inner\n");
    for (stream, expected) in [
        (
            "raw",
            vec![
                ("string-start/tagged=false/multiline=false", false),
                ("string-text", false),
                ("interpolation-start", false),
                ("string-start/tagged=false/multiline=true", false),
                ("string-text", false),
                ("string-end", true),
                ("interpolation-end", true),
                ("string-end", true),
                ("newline", false),
                ("eof", false),
            ],
        ),
        (
            "layout",
            vec![
                ("string-start/tagged=false/multiline=false", false),
                ("string-text", false),
                ("interpolation-start", false),
                ("string-start/tagged=false/multiline=true", false),
                ("string-text", false),
                ("string-end", true),
                ("interpolation-end", true),
                ("string-end", true),
                ("eof", false),
            ],
        ),
    ] {
        let JsonValue::Array(tokens) = root.get(stream).expect("stream") else {
            panic!("stream must be an array");
        };
        let observed = tokens
            .iter()
            .map(|token| {
                let JsonValue::Object(token) = token else {
                    panic!("token must be an object");
                };
                let JsonValue::String(kind) = token.get("kind").expect("kind") else {
                    panic!("kind must be a string");
                };
                let JsonValue::Bool(synthetic) = token.get("synthetic").expect("synthetic") else {
                    panic!("synthetic must be a boolean");
                };
                (kind.as_ref(), *synthetic)
            })
            .collect::<Vec<_>>();
        assert_eq!(observed, expected, "{stream}");
    }
}

#[test]
fn parser_owned_rejections_match_stage0() {
    let session = FrontEndSession::new().expect("front-end session");
    for source in [
        "let value = (1, 2)\n",
        "let value = Array.of<int> (1, 2)\n",
        "record Box { value: int }\nlet base = Box { value: 1 }\nlet next = Box { ...base value: 2 }\n",
        "record Box { value: int }\nlet base = Box { value: 1 }\nlet next = Box { ...base, ...base }\n",
        "(left + right) = value\n",
        "defer 1\n",
        "concurrent { task: run() } else { 0 }\n",
        "concurrent(timeout: 3s) { task: run() }\n",
        "concurrent(timeout 3s) { task: run() } else { 0 }\n",
        "concurrent(timeout: (3)) { task: run() } else { 0 }\n",
        "let mut None = 1\n",
        "const None: Option<int> = None\n",
        "function inspect(None: int) -> int { 1 }\n",
        "let inspect: (int) -> int = None => 1\n",
        "let inspect = (None: int) => 1\n",
        "protocol Inspect { function inspect(None: int) -> int }\n",
        "let value: \"prefix {1}\" = \"prefix 1\"\n",
        "using None = resource { () }\n",
        "match value { case None: Option<int> => 0 }\n",
        "let { None } = value\n",
        "~value\n",
        "let query = unknown\"value\"\n",
        "use tools\n",
        "import \"tools\"\n",
        "export { value }\n",
        "export import tools\n",
        "import tools as ns { value }\n",
        "import tools {}\n",
        "import tools { value, value }\n",
        "import tools { left as value, right as value }\n",
        "import tools { first as x, second as y, third as y, fourth as x, fifth as x }\n",
        "let value = 1\nimport tools\n",
        "export let [head, ..tail] = values\n",
        "type Empty<> = int\n",
        "function identity<>(value: int) -> int { value }\n",
        "enum EmptyPayload { Item() }\n",
        "enum Name = value\n",
        "record Name = value\n",
        "newtype Name { value }\n",
        "impl Name = value\n",
        "protocol Name = value\n",
        "impl Name { let value = 1 }\n",
        "protocol Name { let value = 1 }\n",
        "mut let value = 1\n",
        "let value mut = 1\n",
        "function misplaced(value: int, self) { 0 }\n",
        "function spread(...self) { 0 }\n",
        "export impl Name { function value(self) -> int { 0 } }\n",
        "export enum Name = value\n",
        "export record Name = value\n",
        "export newtype Name { value }\n",
        "let value = 1 }\n",
        "let value = { let x = 1 let y = 2 }\n",
        "let value = concurrent { first: run() second: run() }\n",
        "let value = match input { case _ => 1 case _ => 2 }\n",
        "let input = 1\nlet value = match input {}\n",
        "let input = 1\nlet value = match input {\n  let x = 1\n  case _ => 2\n}\n",
        "function run() -> int { 1 }\nlet value = concurrent {\n  let x = 1\n  second: run()\n}\n",
        "let result = map { for value in [] => value\n",
        "let r = 1..",
        "let r = (1..)\nr",
        "let xs = [1..]\nxs",
        "let r = 1.. by 2\nr",
        "match x {\n  case n if n in 1.. => 0\n  case _ => 1\n}",
        "type Box<T>> = T\n",
        "function identity<T>>(value: T) -> T { value }\n",
        "impl Show<User>> {}\n",
        "record Box derives Eq",
        "record Box { value: int",
        "enum Choice derives Eq",
        "enum Choice { Item",
        "protocol Show<T>",
        "protocol Show<T> {",
        "function run()",
        "function run() {",
        "let { value",
        "let value = match input",
        "let value = match input { case _ => 0",
        "let value = map { key: 1",
        "import tools { value",
        "let = 1\nimport tools\n",
    ] {
        let root = preview_response(&session, source);
        assert_eq!(
            preview_diagnostics(&root),
            rust_frontend_diagnostics(source),
            "{source:?}"
        );
    }

    const EXPLICIT_TYPE_ARGUMENT_COMMA_LIST: &str = "let value = Array.of<int> (1, 2)\n";
    const EXPLICIT_TYPE_ARGUMENT_NOTE: &str =
        "Topaz infers type arguments — drop the explicit `<…>`, e.g. `Array.of(…)`";

    let syntax = preview_source_with(&session, "fixture", EXPLICIT_TYPE_ARGUMENT_COMMA_LIST)
        .expect("self-hosted syntax preview");
    assert_eq!(syntax.diagnostics.len(), 1, "{:?}", syntax.diagnostics);
    assert_eq!(
        syntax.diagnostics[0]
            .notes
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [EXPLICIT_TYPE_ARGUMENT_NOTE],
    );

    let self_preview = typed_source(EXPLICIT_TYPE_ARGUMENT_COMMA_LIST);
    assert_eq!(
        self_preview.resolved.diagnostics.len(),
        1,
        "{:?}",
        self_preview.resolved.diagnostics,
    );
    assert_eq!(
        self_preview.resolved.diagnostics[0]
            .notes
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [EXPLICIT_TYPE_ARGUMENT_NOTE],
    );

    let request = topaz_kernel::KernelRequest::checked(
        "main.tpz",
        Some(""),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    let stage0 = stage0_resolved_unit(
        &SourceFixtureHost(EXPLICIT_TYPE_ARGUMENT_COMMA_LIST),
        request,
    );
    assert_eq!(stage0.resolved.diagnostics.len(), 1);
    assert_eq!(
        stage0.resolved.diagnostics[0]
            .notes
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [EXPLICIT_TYPE_ARGUMENT_NOTE],
    );

    let bundle = topaz_kernel::build_typed_preview_observation(self_preview.observation_input())
        .expect("self-hosted rejected observation");
    let diagnostics = bundle
        .files
        .iter()
        .find(|file| file.path == "diagnostics.jsonl")
        .expect("diagnostics projection");
    let row = json_parse(
        std::str::from_utf8(&diagnostics.bytes)
            .expect("diagnostics UTF-8")
            .trim(),
    )
    .expect("diagnostics JSONL row");
    let JsonValue::Object(row) = row else {
        panic!("diagnostics row must be an object");
    };
    assert_eq!(
        json_string_array_field(&row, "notes", "diagnostics row").expect("diagnostics notes"),
        [EXPLICIT_TYPE_ARGUMENT_NOTE],
    );
}
