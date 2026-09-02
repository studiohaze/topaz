use crate::*;

pub(crate) const STAGE1_RESPONSE_FIELDS: [&str; 8] = [
    "schema",
    "status",
    "frontEnd",
    "loweredModules",
    "loweredOperations",
    "generatedRust",
    "unsupported",
    "provenance",
];

pub(crate) struct Stage1ResponseEnvelope {
    pub(crate) status: String,
    pub(crate) front_end: String,
    pub(crate) front_end_root: Rc<JsonObject>,
    pub(crate) queries: Vec<topaz_kernel::HostQuery>,
}

pub(crate) struct Stage1ResponseControl {
    status: String,
    front_end_root: Rc<JsonObject>,
    queries: Vec<topaz_kernel::HostQuery>,
}

pub(crate) fn decode_stage1_response_root(
    response: &[u8],
    context: &str,
) -> Result<Rc<JsonObject>, String> {
    let text = std::str::from_utf8(response)
        .map_err(|error| format!("{context} is not UTF-8: {error}"))?;
    let parsed = json_parse(text).map_err(|error| format!("{context} is not JSON: {error:?}"))?;
    let root = exact_object(&parsed, context, &STAGE1_RESPONSE_FIELDS)?;
    expect_json_string(root, "schema", STAGE1_EXCHANGE_SCHEMA)?;
    let JsonValue::Object(root) = parsed else {
        unreachable!("exact Stage 1 response object was validated above")
    };
    Ok(root)
}

pub(crate) fn parse_stage1_response_envelope(
    root: &JsonObject,
    context: &str,
) -> Result<Stage1ResponseEnvelope, String> {
    let control = parse_stage1_response_control(root, context)?;
    Ok(Stage1ResponseEnvelope {
        status: control.status,
        front_end: json_string_field(root, "frontEnd")?.to_string(),
        front_end_root: control.front_end_root,
        queries: control.queries,
    })
}

pub(crate) fn parse_stage1_response_control(
    root: &JsonObject,
    context: &str,
) -> Result<Stage1ResponseControl, String> {
    let front_end_context = format!("{context} front-end member");
    let parsed =
        decode_front_end_response_text(json_string_field(root, "frontEnd")?, &front_end_context)?;
    Ok(Stage1ResponseControl {
        status: json_string_field(root, "status")?.to_string(),
        front_end_root: parsed.clone(),
        queries: parse_queries(&parsed)?,
    })
}

pub(crate) fn parse_stage1_lowered_modules(
    root: &JsonObject,
) -> Result<Vec<Stage1LoweredModule>, String> {
    json_array_field(root, "loweredModules")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = exact_object(
                value,
                &format!("Stage 1 lowered module {ordinal}"),
                &[
                    "schema",
                    "identity",
                    "path",
                    "sourceOrdinal",
                    "initializationOrdinal",
                    "entry",
                    "extern",
                    "operationIds",
                ],
            )?;
            expect_json_string(object, "schema", STAGE1_IR_SCHEMA)?;
            let operation_ids = json_array_field(object, "operationIds")?
                .iter()
                .map(|value| match value {
                    JsonValue::String(value) => Ok(value.to_string()),
                    _ => Err("Stage 1 module operation identity is not a string".to_string()),
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(Stage1LoweredModule {
                identity: json_string_field(object, "identity")?.to_string(),
                path: json_string_field(object, "path")?.to_string(),
                source_ordinal: u64::try_from(json_i64(object, "sourceOrdinal")?)
                    .map_err(|_| "Stage 1 source ordinal is negative".to_string())?,
                initialization_ordinal: u64::try_from(json_i64(object, "initializationOrdinal")?)
                    .map_err(|_| {
                    "Stage 1 initialization ordinal is negative".to_string()
                })?,
                entry: json_bool_field(object, "entry")?,
                extern_module: json_bool_field(object, "extern")?,
                operation_ids,
            })
        })
        .collect()
}

pub(crate) fn parse_stage1_operation_string_array(
    object: &JsonObject,
    ordinal: usize,
    field: &str,
) -> Result<Vec<String>, String> {
    json_array_field(object, field)?
        .iter()
        .enumerate()
        .map(|(index, value)| match value {
            JsonValue::String(value) => Ok(value.to_string()),
            _ => Err(format!(
                "Stage 1 operation {ordinal} {field} row {index} is not a string"
            )),
        })
        .collect()
}

pub(crate) fn parse_stage1_lowered_operations(
    root: &JsonObject,
) -> Result<Vec<Stage1LoweredOperation>, String> {
    json_array_field(root, "loweredOperations")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = exact_object(
                value,
                &format!("Stage 1 operation {ordinal}"),
                &[
                    "schema",
                    "id",
                    "module",
                    "lo",
                    "hi",
                    "parentId",
                    "role",
                    "kind",
                    "detail",
                    "operands",
                    "operandLabels",
                    "semanticType",
                    "representation",
                    "referenceIdentity",
                    "bindingName",
                    "bindingMutable",
                    "bindingStorage",
                    "declarationIdentity",
                    "controlKind",
                    "controlTarget",
                    "cleanupIds",
                    "callTarget",
                    "callCalleeKind",
                    "callMethod",
                    "callMethodClass",
                    "callOptional",
                    "callShadowFirst",
                    "callStageMethod",
                    "callArguments",
                    "callEvaluations",
                    "runtimeLeaf",
                    "generatedNameSeed",
                ],
            )?;
            expect_json_string(object, "schema", STAGE1_IR_SCHEMA)?;
            let operands = json_array_field(object, "operands")?
                .iter()
                .enumerate()
                .map(|(operand, value)| match value {
                    JsonValue::String(value) => Ok(value.to_string()),
                    _ => Err(format!(
                        "Stage 1 operation {ordinal} operand {operand} is not a string"
                    )),
                })
                .collect::<Result<Vec<_>, String>>()?;
            let operand_labels =
                parse_stage1_operation_string_array(object, ordinal, "operandLabels")?;
            if operand_labels.len() != operands.len() {
                return Err(format!(
                    "Stage 1 operation {ordinal} has {} operands but {} operand labels",
                    operands.len(),
                    operand_labels.len()
                ));
            }
            let parent = json_string_field(object, "parentId")?;
            Ok(Stage1LoweredOperation {
                id: json_string_field(object, "id")?.to_string(),
                module: json_string_field(object, "module")?.to_string(),
                lo: json_u32(object, "lo")?,
                hi: json_u32(object, "hi")?,
                parent_id: (!parent.is_empty()).then(|| parent.to_string()),
                role: json_string_field(object, "role")?.to_string(),
                kind: json_string_field(object, "kind")?.to_string(),
                detail: json_string_field(object, "detail")?.to_string(),
                operands,
                operand_labels,
                semantic_type: json_string_field(object, "semanticType")?.to_string(),
                representation: json_string_field(object, "representation")?.to_string(),
                reference_identity: json_string_field(object, "referenceIdentity")?.to_string(),
                binding_name: json_string_field(object, "bindingName")?.to_string(),
                binding_mutable: json_bool_field(object, "bindingMutable")?,
                binding_storage: json_string_field(object, "bindingStorage")?.to_string(),
                declaration_identity: json_string_field(object, "declarationIdentity")?.to_string(),
                control_kind: json_string_field(object, "controlKind")?.to_string(),
                control_target: json_string_field(object, "controlTarget")?.to_string(),
                cleanup_ids: parse_stage1_operation_string_array(object, ordinal, "cleanupIds")?,
                call_target: json_string_field(object, "callTarget")?.to_string(),
                call_callee_kind: json_string_field(object, "callCalleeKind")?.to_string(),
                call_method: json_string_field(object, "callMethod")?.to_string(),
                call_method_class: json_string_field(object, "callMethodClass")?.to_string(),
                call_optional: json_bool_field(object, "callOptional")?,
                call_shadow_first: json_bool_field(object, "callShadowFirst")?,
                call_stage_method: json_string_field(object, "callStageMethod")?.to_string(),
                call_arguments: parse_stage1_operation_string_array(
                    object,
                    ordinal,
                    "callArguments",
                )?,
                call_evaluations: parse_stage1_operation_string_array(
                    object,
                    ordinal,
                    "callEvaluations",
                )?,
                runtime_leaf: json_string_field(object, "runtimeLeaf")?.to_string(),
                generated_name_seed: json_string_field(object, "generatedNameSeed")?.to_string(),
            })
        })
        .collect()
}

pub(crate) fn parse_stage1_unsupported(
    root: &JsonObject,
    status: &str,
) -> Result<Vec<String>, String> {
    let unsupported = json_array_field(root, "unsupported")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| match value {
            JsonValue::String(value) => Ok(value.to_string()),
            _ => Err(format!("Stage 1 unsupported row {ordinal} is not a string")),
        })
        .collect::<Result<Vec<_>, String>>()?;
    if (status == "unsupported") == unsupported.is_empty() {
        return Err("Stage 1 unsupported status contradicts its ledger".to_string());
    }
    Ok(unsupported)
}

pub(crate) fn parse_stage1_provenance_source_set_id(
    root: &JsonObject,
    producer: CompilerProducer,
) -> Result<String, String> {
    let provenance = exact_object(
        root.get("provenance")
            .ok_or_else(|| "Stage 1 response omitted provenance".to_string())?,
        "Stage 1 provenance",
        &[
            "schema",
            "engine",
            "producerStage",
            "resultStage",
            "defaultEngine",
            "exchangeSchema",
            "irSchema",
            "sourceSetId",
            "fixedPoint",
        ],
    )?;
    expect_json_string(provenance, "schema", STAGE1_PROVENANCE_SCHEMA)?;
    expect_json_string(provenance, "engine", producer.identity())?;
    expect_json_string(provenance, "defaultEngine", "rust-stage0")?;
    expect_json_string(provenance, "exchangeSchema", STAGE1_EXCHANGE_SCHEMA)?;
    expect_json_string(provenance, "irSchema", STAGE1_IR_SCHEMA)?;
    expect_json_string(provenance, "fixedPoint", "not-run")?;
    if json_i64(provenance, "producerStage")? != producer.stage()
        || json_i64(provenance, "resultStage")? != producer.stage()
    {
        return Err("compiler provenance carries the wrong stage identity".to_string());
    }
    let source_set = json_string_field(provenance, "sourceSetId")?.to_string();
    if source_set != source_set_id() {
        return Err("Stage 1 provenance source-set identity drifted".to_string());
    }
    Ok(source_set)
}

pub(crate) fn advance_compiler_fact_round(
    source: &dyn topaz_kernel::HostFactSource,
    request: &mut topaz_kernel::KernelRequest,
    status: &str,
    queries: Vec<topaz_kernel::HostQuery>,
    context: &str,
) -> Result<bool, String> {
    if status != "need-facts" {
        if !queries.is_empty() {
            return Err(format!("{context} completed with pending fact queries"));
        }
        return Ok(false);
    }
    if queries.is_empty() {
        return Err(format!("{context} requested a fact round without queries"));
    }
    if queries.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(format!(
            "{context} queries are duplicate or out of canonical order"
        ));
    }
    for query in queries {
        if !request
            .mounts()
            .iter()
            .any(|mount| mount.id == query.mount_id())
        {
            return Err(format!(
                "{context} queried unknown mount `{}`",
                query.mount_id()
            ));
        }
        if request.facts().contains_key(&query) {
            return Err(format!("{context} repeated an already answered query"));
        }
        let fact = preview_fact(source, request, &query);
        request
            .supply_fact(query, fact)
            .map_err(|error| format!("{context} supplied invalid fact: {error:?}"))?;
    }
    Ok(true)
}

/// Decodes requested queries, supplies their facts, and reports response completion.
pub fn supply_stage1_response_facts(
    source: &dyn topaz_kernel::HostFactSource,
    request: &mut topaz_kernel::KernelRequest,
    response: &[u8],
) -> Result<bool, String> {
    let root = decode_stage1_response_root(response, "Stage 1 response")?;
    let Stage1ResponseControl {
        status,
        front_end_root: _,
        queries,
    } = parse_stage1_response_control(&root, "Stage 1")?;
    if advance_compiler_fact_round(source, request, &status, queries, "Stage 1")? {
        return Ok(false);
    }
    if status != "completed" {
        return Err(format!("Stage 1 did not complete: status `{status}`"));
    }
    Ok(true)
}

/// Produces a lowered Stage 1 projection with a fresh embedded session.
pub fn preview_stage1_lowered(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<Stage1LoweringPreviewResult, String> {
    preview_stage1_lowered_with(&FrontEndSession::new()?, source, request)
}
