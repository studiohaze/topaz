use super::*;

#[test]
fn stage1_fact_supply_uses_the_shared_response_control_path() {
    let front_end = stage1_encode_json(&stage1_json_object([
        ("ast", stage1_json_array(Vec::new())),
        ("checkerDiagnostics", stage1_json_array(Vec::new())),
        ("declarations", stage1_json_array(Vec::new())),
        ("diagnostics", stage1_json_array(Vec::new())),
        ("edges", stage1_json_array(Vec::new())),
        ("exports", stage1_json_array(Vec::new())),
        ("layout", stage1_json_array(Vec::new())),
        ("modules", stage1_json_array(Vec::new())),
        (
            "queries",
            stage1_json_array(vec![stage1_json_object([
                ("kind", stage1_json_string("read-source")),
                ("logicalPath", stage1_json_string("root/main.tpz")),
                ("mountId", stage1_json_string("root")),
            ])]),
        ),
        ("raw", stage1_json_array(Vec::new())),
        ("references", stage1_json_array(Vec::new())),
        ("resolverDiagnostics", stage1_json_array(Vec::new())),
        ("schema", stage1_json_string(EXCHANGE_SCHEMA)),
        ("scopes", stage1_json_array(Vec::new())),
        ("sourceId", stage1_json_string("")),
        ("status", stage1_json_string("need-facts")),
        ("typedCalls", stage1_json_array(Vec::new())),
        ("typedCaptures", stage1_json_array(Vec::new())),
        ("typedNodes", stage1_json_array(Vec::new())),
    ]));
    let response = stage1_encode_json(&stage1_json_object([
        ("frontEnd", stage1_json_string(&front_end)),
        ("generatedRust", stage1_json_string("")),
        ("loweredModules", stage1_json_array(Vec::new())),
        ("loweredOperations", stage1_json_array(Vec::new())),
        ("provenance", stage1_json_object([])),
        ("schema", stage1_json_string(STAGE1_EXCHANGE_SCHEMA)),
        ("status", stage1_json_string("need-facts")),
        ("unsupported", stage1_json_array(Vec::new())),
    ]));
    let mut request = lowering_request();
    let completed =
        supply_stage1_response_facts(&LoweringFixtureHost, &mut request, response.as_bytes())
            .expect("fact round response is admitted");
    assert!(!completed);
    assert!(
        request
            .facts()
            .contains_key(&topaz_kernel::HostQuery::ReadSource {
                mount_id: "root".to_string(),
                logical_path: "root/main.tpz".to_string(),
            })
    );
}

#[test]
fn stage1_exchange_rejects_wrong_producer_instead_of_falling_back() {
    let session = FrontEndSession::new().expect("checked embedded compiler source");
    let encoded = encode_stage1_request(&lowering_request()).expect("Stage 1 request");
    let text = String::from_utf8(encoded).expect("UTF-8 request");
    let wrong = text.replacen("topaz-stage1", "rust-stage0", 1);
    let error = session
        .invoke_stage1(wrong.as_bytes())
        .expect_err("wrong producer must stop");
    assert!(error.contains("unsupported compiler producer"), "{error}");
}

#[test]
fn stage1_exchange_rejects_unadmitted_language_mode() {
    let session = FrontEndSession::new().expect("checked embedded compiler source");
    let encoded = encode_stage1_request(&lowering_request()).expect("Stage 1 request");
    let text = String::from_utf8(encoded).expect("UTF-8 request");
    let wrong = text.replacen("topaz-5.20", "topaz-5.21", 1);
    let error = session
        .invoke_stage1(wrong.as_bytes())
        .expect_err("unadmitted language mode must stop");
    assert!(
        error.contains("unsupported self-hosted language mode `topaz-5.21`"),
        "{error}"
    );
}

#[test]
fn stage1_lowering_covers_the_locked_bootstrap_profile() {
    let request = topaz_kernel::KernelRequest::checked(
        "src/main.tpz",
        Some(""),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Lowered);
    let stage1 = preview_stage1_lowered(&BootstrapFixtureHost::new(), request.clone())
        .expect("Stage 1 bootstrap lowering");
    assert_eq!(stage1.status, "completed", "{:#?}", stage1.unsupported);
    assert!(stage1.unsupported.is_empty());

    let stage0 = topaz_kernel::drive_checked(&BootstrapFixtureHost::new(), request);
    let stage0_operations = match &stage0.outcome {
        topaz_kernel::KernelOutcome::Completed(unit) => unit
            .lowered
            .as_ref()
            .expect("Stage 0 lowered unit")
            .operations
            .iter()
            .map(|operation| (operation.id.clone(), operation.operands.clone()))
            .collect::<std::collections::BTreeMap<_, _>>(),
        _ => panic!("unexpected Stage 0 bootstrap outcome"),
    };
    let stage1_operations = stage1
        .operations
        .iter()
        .map(|operation| (operation.id.clone(), operation.operands.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let missing = stage0_operations
        .keys()
        .filter(|id| !stage1_operations.contains_key(*id))
        .collect::<Vec<_>>();
    let extra = stage1_operations
        .keys()
        .filter(|id| !stage0_operations.contains_key(*id))
        .collect::<Vec<_>>();
    let operand_mismatches = stage0_operations
        .iter()
        .filter_map(|(id, expected)| {
            let actual = stage1_operations.get(id)?;
            (actual != expected).then_some((id, expected, actual))
        })
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty() && extra.is_empty() && operand_mismatches.is_empty(),
        "missing={missing:#?}\nextra={extra:#?}\noperand_mismatches={operand_mismatches:#?}"
    );
}

#[test]
fn stage1_lowering_treats_protocol_signatures_as_declaration_metadata() {
    let session = FrontEndSession::new().expect("checked embedded compiler source");
    let host = InlineLoweringFixtureHost(concat!(
        "protocol P {\n",
        "  function f(value: Self, other: Option<Self>) -> Self\n",
        "}\n",
        "0\n",
    ));
    let lowered = preview_stage1_lowered_with(&session, &host, lowering_request())
        .expect("Stage 1 protocol declaration lowering");
    assert_eq!(
        (
            lowered.status.as_str(),
            lowered.unsupported.as_slice(),
            lowered
                .operations
                .iter()
                .filter(|operation| operation.kind == "protocol")
                .count(),
        ),
        ("completed", &[] as &[String], 1),
    );
}

#[test]
fn stage1_lowering_preserves_declared_control_and_callable_operand_order() {
    let session = FrontEndSession::new().expect("checked embedded compiler source");
    for source in [
        concat!(
            "function apply(value: int, f: (int) -> int) -> int { f(value) }\n",
            "let offset = 2\n",
            "let add = (value: int) => value + offset\n",
            "let result = apply(40, add)\n",
        ),
        concat!(
            "function add(left: int, right: int) -> int { left + right }\n",
            "let result = 40 |> add(2)\n",
        ),
        concat!(
            "function answer(base: int = 40, offset: int = 2) -> int { base + offset }\n",
            "let result = answer()\n",
        ),
        concat!(
            "record Person { prefix: string }\n",
            "impl Person {\n",
            "  function greet(self, name: string = \"Ada\", suffix: string = \"!\") -> string {\n",
            "    \"{self.prefix} {name}{suffix}\"\n",
            "  }\n",
            "}\n",
            "let person = Person { prefix: \"hi\" }\n",
            "let result = person.greet(suffix: \"?\")\n",
        ),
        concat!(
            "let values: Option<Array<int>> = Some([42])\n",
            "let result = values?.get(0)\n",
        ),
        concat!(
            "let values: Array<int> | null = [42]\n",
            "let result = values?.get(0)\n",
        ),
        concat!(
            "record Runner { run: (int) -> int }\n",
            "let runner: Option<Runner> = Some(Runner {\n",
            "  run: (value: int) => value + 1,\n",
            "})\n",
            "let result = runner?.run(41)\n",
        ),
        concat!(
            "function invoke<T>(value: Option<T>) {\n",
            "  let result = value?.run(41)\n",
            "}\n",
            "let sentinel = 0\n",
        ),
        concat!(
            "function score(prefix: int, suffix: int, ...rest: int) -> int {\n",
            "  prefix * 100 + suffix * 10 + rest.length\n",
            "}\n",
            "let values = [40]\n",
            "let member = values.get(0)\n",
            "let result = score(suffix: 2, prefix: 1) + score(3, 5, ...[7, 8], 9)\n",
        ),
        concat!(
            "let values = [1, 2, 3]\n",
            "let results = for value in values { value + 1 }\n",
        ),
        concat!(
            "let selected = Some(41)\n",
            "let result = match selected {\n",
            "  case Some(value) => value + 1\n",
            "  case None => 0\n",
            "}\n",
        ),
        concat!(
            "enum Token { Plus, Star }\n",
            "function apply(token: Token) -> int {\n",
            "  match token {\n",
            "    case Plus => 1\n",
            "    case Star => 2\n",
            "  }\n",
            "}\n",
            "let result = apply(Token.Star)\n",
        ),
        concat!(
            "record Pair { left: int, right: int }\n",
            "let pair = Pair { left: 20, right: 22 }\n",
            "let result = pair.left + pair.right\n",
        ),
        concat!(
            "let mut value = 40\n",
            "value += 2\n",
            "let result = value\n",
        ),
        "let answer: int = 42\n",
    ] {
        let host = InlineLoweringFixtureHost(source);
        let request = lowering_request();
        let stage1 = preview_stage1_lowered_with(&session, &host, request.clone())
            .expect("Stage 1 declared lowering");
        assert_eq!(stage1.status, "completed", "{:#?}", stage1.unsupported);
        assert!(stage1.unsupported.is_empty());
        let stage0 = topaz_kernel::drive_checked(&host, request);
        let (stage0_operations, stage0_runtime, stage0_bindings, stage0_representations) =
            match &stage0.outcome {
                topaz_kernel::KernelOutcome::Completed(unit) => {
                    let operations = &unit
                        .lowered
                        .as_ref()
                        .expect("Stage 0 lowered unit")
                        .operations;
                    (
                        operations
                            .iter()
                            .map(|operation| {
                                (
                                    operation.id.clone(),
                                    (operation.parent.clone(), operation.operands.clone()),
                                )
                            })
                            .collect::<std::collections::BTreeMap<_, _>>(),
                        operations
                            .iter()
                            .map(|operation| {
                                (
                                    operation.id.clone(),
                                    operation.runtime_leaf.clone().unwrap_or_default(),
                                )
                            })
                            .collect::<std::collections::BTreeMap<_, _>>(),
                        operations
                            .iter()
                            .map(|operation| {
                                (
                                    operation.id.clone(),
                                    operation.binding.as_ref().map(|binding| {
                                        (
                                            binding.name.clone(),
                                            binding.mutable,
                                            match binding.storage {
                                                topaz_hir::LoweredStorage::Local => "local",
                                                topaz_hir::LoweredStorage::Module => "module",
                                                topaz_hir::LoweredStorage::Captured => "captured",
                                                topaz_hir::LoweredStorage::Parameter => "parameter",
                                                topaz_hir::LoweredStorage::Temporary => "temporary",
                                            },
                                            binding
                                                .declaration_identity
                                                .clone()
                                                .unwrap_or_default(),
                                        )
                                    }),
                                )
                            })
                            .collect::<std::collections::BTreeMap<_, _>>(),
                        operations
                            .iter()
                            .map(|operation| {
                                (
                                    operation.id.clone(),
                                    operation
                                        .representation
                                        .map(topaz_hir::MonoTy::name)
                                        .unwrap_or_default(),
                                )
                            })
                            .collect::<std::collections::BTreeMap<_, _>>(),
                    )
                }
                _ => panic!("unexpected Stage 0 declared lowering outcome for {source:?}"),
            };
        let lowered_call_plans = |bytes: &[u8]| {
            std::str::from_utf8(bytes)
                .expect("lowered projection UTF-8")
                .lines()
                .filter_map(|line| {
                    let JsonValue::Object(row) = json_parse(line).expect("lowered projection JSON")
                    else {
                        panic!("lowered projection row must be an object")
                    };
                    if json_string_field(&row, "rowKind").expect("lowered row kind") != "operation"
                    {
                        return None;
                    }
                    Some((
                        json_string_field(&row, "operationId")
                            .expect("lowered operation identity")
                            .to_string(),
                        row.get("call").expect("lowered call field").clone(),
                    ))
                })
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        let stage0_observation =
            topaz_kernel::build_observation(&stage0).expect("Stage 0 lowered observation");
        let stage0_call_plans = lowered_call_plans(
            &stage0_observation
                .files
                .iter()
                .find(|file| file.path == "lowered.jsonl")
                .expect("Stage 0 lowered observation file")
                .bytes,
        );
        let stage1_call_plans = lowered_call_plans(
            &encode_stage1_lowered_projection(&stage1)
                .expect("current self-host lowered projection"),
        );
        assert_eq!(stage1_call_plans, stage0_call_plans, "{source:?}");
        let stage1_operations = stage1
            .operations
            .iter()
            .map(|operation| {
                (
                    operation.id.clone(),
                    (operation.parent_id.clone(), operation.operands.clone()),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(stage1_operations, stage0_operations, "{source:?}");
        let stage1_runtime = stage1
            .operations
            .iter()
            .map(|operation| (operation.id.clone(), operation.runtime_leaf.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(stage1_runtime, stage0_runtime, "{source:?}");
        let stage1_bindings = stage1
            .operations
            .iter()
            .map(|operation| {
                (
                    operation.id.clone(),
                    (!operation.binding_name.is_empty()).then(|| {
                        (
                            operation.binding_name.clone(),
                            operation.binding_mutable,
                            operation.binding_storage.as_str(),
                            operation.declaration_identity.clone(),
                        )
                    }),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(stage1_bindings, stage0_bindings, "{source:?}");
        let stage1_representations = stage1
            .operations
            .iter()
            .map(|operation| (operation.id.clone(), operation.representation.as_str()))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(stage1_representations, stage0_representations, "{source:?}");
        assert!(stage1.operations.iter().all(|operation| {
            operation.generated_name_seed == operation.id
                && operation.operand_labels.len() == operation.operands.len()
                && (operation.binding_name.is_empty()
                    || (!operation.binding_storage.is_empty()
                        && !operation.declaration_identity.is_empty()))
        }));
        for operation in stage1
            .operations
            .iter()
            .filter(|operation| operation.kind == "expression/call")
        {
            if operation.call_callee_kind.is_empty() {
                let parent = operation
                    .parent_id
                    .as_ref()
                    .and_then(|parent_id| {
                        stage1
                            .operations
                            .iter()
                            .find(|candidate| candidate.id == *parent_id)
                    })
                    .expect("a stage call without its own plan has a pipeline parent");
                assert_eq!(parent.kind, "expression/pipeline", "{source:?}");
                assert_eq!(parent.call_callee_kind, "pipe", "{source:?}");
                assert!(!parent.call_evaluations.is_empty(), "{source:?}");
            } else {
                assert!(!operation.call_evaluations.is_empty(), "{source:?}");
            }
        }
        for operation in stage1.operations.iter().filter(|operation| {
            matches!(
                operation.kind.as_str(),
                "expression/if"
                    | "expression/match"
                    | "expression/for"
                    | "expression/loop"
                    | "statement/while"
            )
        }) {
            assert!(!operation.control_kind.is_empty(), "{source:?}");
            assert_eq!(operation.control_target, operation.id, "{source:?}");
        }
        for operation in stage1
            .operations
            .iter()
            .filter(|operation| operation.kind == "expression/record-literal")
        {
            assert!(
                operation
                    .operand_labels
                    .iter()
                    .any(|label| label.contains("field-initializer") && label.contains("left")),
                "{source:?}: {operation:#?}"
            );
        }
        for operation in stage1
            .operations
            .iter()
            .filter(|operation| operation.kind == "assignment")
        {
            assert_eq!(operation.detail, "add", "{source:?}: {operation:#?}");
        }
    }
}

#[test]
fn stage1_lowering_keeps_checker_rejection_source_free_and_fail_closed() {
    let request = topaz_kernel::KernelRequest::checked(
        "main.tpz",
        Some(""),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::Lowered);
    let result = preview_stage1_lowered(&TypeMismatchFixtureHost, request)
        .expect("Stage 1 rejected lowering result");
    assert_eq!(result.status, "rejected");
    assert!(result.operations.is_empty());
    assert!(result.unsupported.is_empty());
    assert_eq!(result.provenance_source_set_id, source_set_id());
}

#[test]
fn stage1_topaz_emitter_is_source_free_and_executable() {
    let source_text = "let marker = \"topaz_parser\"\nlet answer = 40 + 2\n";
    let host = InlineLoweringFixtureHost(source_text);
    let request = topaz_kernel::KernelRequest::checked(
        "root/main.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::RustSource);
    let session = FrontEndSession::new().expect("checked embedded compiler source");
    let generated =
        preview_stage1_generated_with(&session, &host, request).expect("Topaz Stage 1 generation");
    assert_eq!(generated.status, "completed");
    assert_eq!(generated.provenance_source_set_id, source_set_id());
    assert!(!generated.generated_rust.contains(source_text.trim()));
    assert!(generated.generated_rust.contains("topaz_parser"));
    assert!(generated.generated_rust.contains("Some(42i64)"));
    assert!(
        generated
            .generated_rust
            .contains("topaz.compiler.fixed-point-ir-payload/v1")
    );
    assert!(!generated.generated_rust.contains("\"producerStage\""));
    assert!(!generated.generated_rust.contains("\"resultStage\""));

    let root = std::env::temp_dir().join(format!(
        "topaz-stage1-emitter-verification-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove stale Stage 1 emitter verification");
    }
    std::fs::create_dir_all(&root).expect("create Stage 1 emitter verification");
    let generated_path = root.join("generated.rs");
    let main_path = root.join("main.rs");
    let binary_path = root.join("stage1-emitter-verification");
    std::fs::write(&generated_path, &generated.generated_rust)
        .expect("write generated Stage 1 Rust");
    std::fs::write(
            &main_path,
            format!(
                "mod generated {{ include!({:?}); }}\nfn main() {{ print!(\"{{}}\", generated::compiler_preview_i64().expect(\"preview\")); }}\n",
                generated_path
            ),
        )
        .expect("write Stage 1 emitter verification");
    let compile = std::process::Command::new("rustc")
        .arg("--edition=2024")
        .arg(&main_path)
        .arg("-o")
        .arg(&binary_path)
        .output()
        .expect("run rustc for Stage 1 emitter verification");
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let output = std::process::Command::new(&binary_path)
        .output()
        .expect("run Stage 1 emitter verification");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"42");
    std::fs::remove_dir_all(root).expect("remove Stage 1 emitter verification");
}

#[test]
fn stage1_and_stage2_provenance_share_canonical_generated_source() {
    let host = InlineLoweringFixtureHost("export function main() -> int { 42 }\n");
    let request = topaz_kernel::KernelRequest::checked(
        "root/main.tpz",
        Some("root"),
        LangVersion::CURRENT,
        topaz_kernel::PackageFacts::standalone(),
    )
    .with_terminal_phase(topaz_kernel::TerminalPhase::RustSource);
    let session = FrontEndSession::new().expect("checked embedded compiler source");
    let stage1 = preview_stage1_generated_with(&session, &host, request.clone())
        .expect("Stage 1 generation");
    let stage2 =
        preview_stage2_generated_with(&session, &host, request).expect("Stage 2 generation");
    assert_eq!(stage1.producer, CompilerProducer::Stage1);
    assert_eq!(stage2.producer, CompilerProducer::Stage2);
    assert_eq!(stage1.front_end, stage2.front_end);
    assert_eq!(stage1.generated_rust, stage2.generated_rust);
    assert!(!stage1.generated_rust.contains("\"engine\""));
    assert!(!stage1.generated_rust.contains("\"producerStage\""));
    assert!(!stage1.generated_rust.contains("\"resultStage\""));
}
