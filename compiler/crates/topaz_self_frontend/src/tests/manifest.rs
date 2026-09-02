use super::*;

#[test]
fn stage1_product_manifest_rejects_a_stage2_result() {
    let result = Stage1GeneratedPreviewResult {
        producer: CompilerProducer::Stage2,
        request: generated_request(),
        status: "completed".to_string(),
        profile: CompilationProfile::None,
        front_end: String::new(),
        generated_rust: "fn main() {}".to_string(),
        provenance_source_set_id: format!("sha256:{}", "0".repeat(64)),
        rounds: 1,
        response: Vec::new(),
        response_root: Rc::new(JsonObject::new()),
        front_end_root: Rc::new(JsonObject::new()),
        resolved_diagnostic_shape: ResolvedDiagnosticShape::Current,
    };
    let error = encode_stage1_product_manifest(&result, &format!("sha256:{}", "1".repeat(64)))
        .expect_err("Stage 2 output must not be labeled as a Stage 1 product");
    assert_eq!(error, "Stage 1 product requires the Stage 1 producer");
}

#[test]
fn self_product_manifest_rejects_omission_fallback_and_order_mutations() {
    let host = InlineLoweringFixtureHost("let answer = 42\n");
    let product = preview_linked_stage2_compilation_product(
        &host,
        generated_request(),
        CompilationProfile::None,
    )
    .expect("complete self product");
    let manifest = encode_self_compilation_product_manifest(&product).expect("product manifest");
    let text = std::str::from_utf8(&manifest).expect("manifest UTF-8");
    let omitted = text
        .replace(&format!("\"resultId\":\"{}\",", product.result_id), "")
        .into_bytes();
    assert!(
        validate_self_compilation_product_manifest(&omitted)
            .expect_err("omitted result identity")
            .contains("fields")
    );
    let fallback = text
        .replace(
            "\"targetCompilerFallback\":false",
            "\"targetCompilerFallback\":true",
        )
        .into_bytes();
    assert!(
        validate_self_compilation_product_manifest(&fallback)
            .expect_err("fallback mutation")
            .contains("fallback")
    );
    let uppercase_front_end = format!(
        "sha256:{}",
        product.front_end_sha256["sha256:".len()..].to_ascii_uppercase()
    );
    let noncanonical_hash = text
        .replace(&product.front_end_sha256, &uppercase_front_end)
        .into_bytes();
    assert!(
        validate_self_compilation_product_manifest(&noncanonical_hash)
            .expect_err("uppercase digest mutation")
            .contains("canonical")
    );
    let reordered = text
        .replace(
            "\"self.c2-front-end\",\"self.c2-profile\"",
            "\"self.c2-profile\",\"self.c2-front-end\"",
        )
        .into_bytes();
    assert!(
        validate_self_compilation_product_manifest(&reordered)
            .expect_err("phase order mutation")
            .contains("contract order")
    );
}

#[test]
fn stage1_lowering_basis_matches_stage0_operation_identity_without_fallback() {
    let stage1 = preview_stage1_lowered(&LoweringFixtureHost, lowering_request())
        .expect("Stage 1 lowering basis");
    assert_eq!(stage1.status, "completed");
    assert!(stage1.unsupported.is_empty());
    assert!(stage1.rounds > 1);
    assert_eq!(stage1.provenance_source_set_id, source_set_id());
    assert!(
        stage1
            .operations
            .iter()
            .any(|operation| operation.kind == "expression/binary"
                && operation.detail == "add"
                && operation.runtime_leaf == "binary"
                && operation.operands.len() == 2)
    );
    assert!(
        stage1
            .operations
            .iter()
            .any(|operation| operation.kind == "pattern/binding"
                && operation.binding_name == "answer"
                && operation.binding_storage == "local"
                && !operation.declaration_identity.is_empty())
    );
    assert!(stage1.operations.iter().all(|operation| {
        operation.generated_name_seed == operation.id
            && operation.operand_labels.len() == operation.operands.len()
            && matches!(
                operation.representation.as_str(),
                "" | "i64"
                    | "f64"
                    | "bool"
                    | "unit"
                    | "bytes-handle"
                    | "byte-buffer-handle"
                    | "boxed"
            )
    }));

    let stage0 = topaz_kernel::drive_checked(&LoweringFixtureHost, lowering_request());
    let stage0_ids = match &stage0.outcome {
        topaz_kernel::KernelOutcome::Completed(unit) => unit
            .lowered
            .as_ref()
            .expect("Stage 0 lowered unit")
            .operations
            .iter()
            .map(|operation| operation.id.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        _ => panic!("unexpected Stage 0 outcome"),
    };
    let stage1_ids = stage1
        .operations
        .iter()
        .map(|operation| operation.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(stage1_ids, stage0_ids);

    let stage0_operands = match &stage0.outcome {
        topaz_kernel::KernelOutcome::Completed(unit) => unit
            .lowered
            .as_ref()
            .expect("Stage 0 lowered unit")
            .operations
            .iter()
            .map(|operation| (operation.id.clone(), operation.operands.clone()))
            .collect::<std::collections::BTreeMap<_, _>>(),
        _ => panic!("unexpected Stage 0 outcome"),
    };
    let stage1_operands = stage1
        .operations
        .iter()
        .map(|operation| (operation.id.clone(), operation.operands.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(stage1_operands, stage0_operands);
}
