use crate::*;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Stable IDs link parents and operands; call-plan facts stay beside the operation.
pub struct Stage1LoweredOperation {
    pub id: String,
    pub module: String,
    pub lo: u32,
    pub hi: u32,
    pub parent_id: Option<String>,
    pub role: String,
    pub kind: String,
    pub detail: String,
    pub operands: Vec<String>,
    pub operand_labels: Vec<String>,
    pub semantic_type: String,
    pub representation: String,
    pub reference_identity: String,
    pub binding_name: String,
    pub binding_mutable: bool,
    pub binding_storage: String,
    pub declaration_identity: String,
    pub control_kind: String,
    pub control_target: String,
    pub cleanup_ids: Vec<String>,
    pub call_target: String,
    pub call_callee_kind: String,
    pub call_method: String,
    pub call_method_class: String,
    pub call_optional: bool,
    pub call_shadow_first: bool,
    pub call_stage_method: String,
    pub call_arguments: Vec<String>,
    pub call_evaluations: Vec<String>,
    pub runtime_leaf: String,
    pub generated_name_seed: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Source and initialization order remain distinct for runtime module setup.
pub struct Stage1LoweredModule {
    pub identity: String,
    pub path: String,
    pub source_ordinal: u64,
    pub initialization_ordinal: u64,
    pub entry: bool,
    pub extern_module: bool,
    pub operation_ids: Vec<String>,
}

/// Retains the source-set provenance beside the lowered rows used to emit Rust.
pub struct Stage1LoweringPreviewResult {
    pub request: topaz_kernel::KernelRequest,
    pub status: String,
    pub front_end: String,
    pub modules: Vec<Stage1LoweredModule>,
    pub operations: Vec<Stage1LoweredOperation>,
    pub unsupported: Vec<String>,
    pub generated_rust: String,
    pub provenance_source_set_id: String,
    pub rounds: u64,
}

pub(crate) fn decode_stage1_lowered_response(
    root: &JsonObject,
    request: topaz_kernel::KernelRequest,
    status: String,
    front_end: String,
    accept_generated_rust: bool,
    producer: CompilerProducer,
    rounds: u64,
) -> Result<Stage1LoweringPreviewResult, String> {
    if !matches!(status.as_str(), "completed" | "rejected" | "unsupported") {
        return Err(format!("Stage 1 returned invalid basis status `{status}`"));
    }
    let generated_rust = json_string_field(root, "generatedRust")?.to_string();
    if !accept_generated_rust && !generated_rust.is_empty() {
        return Err("Stage 1 lowering basis returned generated Rust too early".to_string());
    }
    let modules = parse_stage1_lowered_modules(root)?;
    let operations = parse_stage1_lowered_operations(root)?;
    if operations.len() as u64 > request.budgets().max_lowered_nodes {
        return Err(format!(
            "Stage 1 lowered-node resource limit: observed {}, limit {}",
            operations.len(),
            request.budgets().max_lowered_nodes
        ));
    }
    let unsupported = parse_stage1_unsupported(root, &status)?;
    let provenance_source_set_id = parse_stage1_provenance_source_set_id(root, producer)?;
    Ok(Stage1LoweringPreviewResult {
        request,
        status,
        front_end,
        modules,
        operations,
        unsupported,
        generated_rust,
        provenance_source_set_id,
        rounds,
    })
}

pub(crate) fn preview_stage1_lowered_by(
    invoke: impl Fn(&[u8]) -> Result<Vec<u8>, String>,
    source: &dyn topaz_kernel::HostFactSource,
    mut request: topaz_kernel::KernelRequest,
    accept_generated_rust: bool,
    producer: CompilerProducer,
    profile: CompilationProfile,
) -> Result<Stage1LoweringPreviewResult, String> {
    if request.terminal_phase() != topaz_kernel::TerminalPhase::Lowered {
        return Err("Stage 1 lowering preview requires the lowered terminal phase".to_string());
    }
    let max_rounds = request
        .budgets()
        .max_source_facts
        .saturating_mul(3)
        .saturating_add(4);
    let mut rounds = 0u64;
    loop {
        if rounds >= max_rounds {
            return Err(format!("Stage 1 fact rounds exceed {max_rounds}"));
        }
        rounds += 1;
        let encoded = encode_compiler_request_with_profile(&request, producer, profile)?;
        let response = invoke(&encoded)?;
        let response_root = decode_stage1_response_root(&response, "Stage 1 response")?;
        let root = response_root.as_ref();
        let Stage1ResponseEnvelope {
            status,
            front_end: front_end_text,
            front_end_root: _,
            queries,
        } = parse_stage1_response_envelope(root, "Stage 1")?;
        if advance_compiler_fact_round(source, &mut request, &status, queries, "Stage 1")? {
            continue;
        }
        return decode_stage1_lowered_response(
            root,
            request,
            status,
            front_end_text,
            accept_generated_rust,
            producer,
            rounds,
        );
    }
}

/// Produces a lowered Stage 1 projection through a reusable session.
pub fn preview_stage1_lowered_with(
    session: &FrontEndSession,
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<Stage1LoweringPreviewResult, String> {
    preview_stage1_lowered_by(
        |encoded| session.invoke_stage1(encoded),
        source,
        request,
        false,
        CompilerProducer::Stage1,
        CompilationProfile::None,
    )
}

/// Runs the embedded C1 image and decodes its lowered projection.
pub fn preview_linked_stage1_lowered(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<Stage1LoweringPreviewResult, String> {
    preview_stage1_lowered_by(
        topaz_stage1_runtime::execute_embedded_compiler,
        source,
        request,
        false,
        CompilerProducer::Stage1,
        CompilationProfile::None,
    )
}

/// Runs the embedded C2 image and decodes its lowered projection.
pub fn preview_linked_stage2_lowered(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<Stage1LoweringPreviewResult, String> {
    preview_stage1_lowered_by(
        topaz_stage1_runtime::execute_embedded_stage2_compiler,
        source,
        request,
        false,
        CompilerProducer::Stage2,
        CompilationProfile::None,
    )
}

/// Rejects an empty profile before invoking C2 and decoding its lowered rows.
pub fn preview_linked_stage2_profiled_lowered(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
    profile: CompilationProfile,
) -> Result<Stage1LoweringPreviewResult, String> {
    if profile == CompilationProfile::None {
        return Err("profiled self compilation requires a named profile".to_string());
    }
    preview_stage1_lowered_by(
        topaz_stage1_runtime::execute_embedded_stage2_compiler,
        source,
        request,
        false,
        CompilerProducer::Stage2,
        profile,
    )
}

/// Uses the retained response tree so typed and lowered views cannot parse different bytes.
pub fn decode_stage1_lowering_from_generated(
    result: &Stage1GeneratedPreviewResult,
) -> Result<Stage1LoweringPreviewResult, String> {
    if result.status == "need-facts" || !parse_queries(&result.front_end_root)?.is_empty() {
        return Err("generated Stage 1 response is not final".to_string());
    }
    decode_stage1_lowered_response(
        &result.response_root,
        result
            .request
            .clone()
            .with_terminal_phase(topaz_kernel::TerminalPhase::Lowered),
        result.status.clone(),
        result.front_end.clone(),
        true,
        result.producer,
        result.rounds,
    )
}

pub(crate) fn stage1_lowered_call_argument(
    operation: &Stage1LoweredOperation,
    source_id: &str,
    ordinal: usize,
    encoded: &str,
) -> Result<JsonValue, String> {
    // The self emitter carries `kind|name|sourceIndex|lo|hi`; every field is mandatory.
    let mut fields = encoded.split('|');
    let (Some(binding_kind), Some(binding_name), Some(source_index), Some(lo), Some(hi)) = (
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
    ) else {
        return Err(format!(
            "Stage 1 operation `{}` call argument {ordinal} has invalid field count",
            operation.id
        ));
    };
    if fields.next().is_some() {
        return Err(format!(
            "Stage 1 operation `{}` call argument {ordinal} has invalid field count",
            operation.id
        ));
    }
    let binding = match binding_kind {
        "positional" | "spread" | "inserted-lead" => {
            stage1_json_object([("kind", stage1_json_string(binding_kind))])
        }
        "named" => stage1_json_object([
            ("kind", stage1_json_string("named")),
            ("name", stage1_json_string(binding_name)),
        ]),
        other => {
            return Err(format!(
                "Stage 1 operation `{}` call argument {ordinal} has unknown binding `{other}`",
                operation.id
            ));
        }
    };
    let source_index = source_index.parse::<i64>().map_err(|error| {
        format!(
            "Stage 1 operation `{}` call argument {ordinal} has invalid source index: {error}",
            operation.id
        )
    })?;
    let source_index = match source_index {
        -1 => JsonValue::Null,
        value if value >= 0 => stage1_json_number(value as u64),
        _ => {
            return Err(format!(
                "Stage 1 operation `{}` call argument {ordinal} has negative source index",
                operation.id
            ));
        }
    };
    let parse_span = |field: &str, value: &str| {
        value.parse::<u64>().map_err(|error| {
            format!(
                "Stage 1 operation `{}` call argument {ordinal} has invalid {field}: {error}",
                operation.id
            )
        })
    };
    Ok(stage1_json_object([
        ("binding", binding),
        ("sourceIndex", source_index),
        (
            "span",
            stage1_json_object([
                ("hi", stage1_json_number(parse_span("span end", hi)?)),
                ("lo", stage1_json_number(parse_span("span start", lo)?)),
                ("sourceId", stage1_json_string(source_id)),
            ]),
        ),
    ]))
}

pub(crate) fn stage1_lowered_call_evaluation(
    operation: &Stage1LoweredOperation,
    ordinal: usize,
    encoded: &str,
) -> Result<JsonValue, String> {
    let Some((kind, argument_index)) = encoded.split_once('|') else {
        return Err(format!(
            "Stage 1 operation `{}` call evaluation {ordinal} has invalid field count",
            operation.id
        ));
    };
    match kind {
        "callee" | "receiver" | "optional-guard" | "pipe-lead" => {
            Ok(stage1_json_object([("kind", stage1_json_string(kind))]))
        }
        "argument" => Ok(stage1_json_object([
            (
                "argumentIndex",
                stage1_json_number(argument_index.parse::<u64>().map_err(|error| {
                    format!(
                        "Stage 1 operation `{}` call evaluation {ordinal} has invalid argument index: {error}",
                        operation.id
                    )
                })?),
            ),
            ("kind", stage1_json_string("argument")),
        ])),
        other => Err(format!(
            "Stage 1 operation `{}` call evaluation {ordinal} has unknown kind `{other}`",
            operation.id
        )),
    }
}

pub(crate) fn stage1_lowered_call_plan(
    operation: &Stage1LoweredOperation,
    source_id: &str,
) -> Result<JsonValue, String> {
    if operation.call_callee_kind.is_empty() {
        return Ok(JsonValue::Null);
    }
    let callee = match operation.call_callee_kind.as_str() {
        "value" => stage1_json_object([("kind", stage1_json_string("value"))]),
        "member" => stage1_json_object([
            ("class", stage1_json_string(&operation.call_method_class)),
            ("kind", stage1_json_string("member")),
            ("method", stage1_json_string(&operation.call_method)),
            ("optional", JsonValue::Bool(operation.call_optional)),
            ("shadowFirst", JsonValue::Bool(operation.call_shadow_first)),
        ]),
        "pipe" => stage1_json_object([
            ("kind", stage1_json_string("pipe")),
            (
                "stageMethod",
                if operation.call_stage_method.is_empty() {
                    JsonValue::Null
                } else {
                    stage1_json_string(&operation.call_stage_method)
                },
            ),
        ]),
        other => {
            return Err(format!(
                "Stage 1 operation `{}` has unknown call callee kind `{other}`",
                operation.id
            ));
        }
    };
    let arguments = operation
        .call_arguments
        .iter()
        .enumerate()
        .map(|(ordinal, encoded)| {
            stage1_lowered_call_argument(operation, source_id, ordinal, encoded)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let evaluation = operation
        .call_evaluations
        .iter()
        .enumerate()
        .map(|(ordinal, encoded)| stage1_lowered_call_evaluation(operation, ordinal, encoded))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(stage1_json_object([
        ("arguments", stage1_json_array(arguments)),
        (
            "binding",
            stage1_json_object([("kind", stage1_json_string("runtime"))]),
        ),
        ("callee", callee),
        ("evaluation", stage1_json_array(evaluation)),
    ]))
}

/// Encodes the canonical lowered JSONL projection of a completed result.
pub fn encode_stage1_lowered_projection(
    result: &Stage1LoweringPreviewResult,
) -> Result<Vec<u8>, String> {
    if result.status != "completed" || !result.unsupported.is_empty() {
        return Err("Stage 1 lowered projection requires a completed admitted result".to_string());
    }
    let sources = result
        .modules
        .iter()
        .map(|module| {
            (
                module.identity.clone(),
                stage1_source_id(&module.identity, &module.path),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut rows = Vec::new();
    for module in &result.modules {
        rows.push(stage1_json_object([
            ("entry", JsonValue::Bool(module.entry)),
            ("extern", JsonValue::Bool(module.extern_module)),
            ("identity", stage1_json_string(&module.identity)),
            (
                "initializationOrdinal",
                stage1_json_number(module.initialization_ordinal),
            ),
            (
                "operationIds",
                stage1_json_array(
                    module
                        .operation_ids
                        .iter()
                        .map(|value| stage1_json_string(value))
                        .collect(),
                ),
            ),
            ("path", stage1_json_string(&module.path)),
            ("rowKind", stage1_json_string("module")),
            ("schema", stage1_json_string(topaz_kernel::LOWERED_SCHEMA)),
            (
                "sourceId",
                stage1_json_string(
                    sources
                        .get(&module.identity)
                        .ok_or_else(|| "Stage 1 module source identity is missing".to_string())?,
                ),
            ),
        ]));
    }
    let leaves = result
        .operations
        .iter()
        .filter_map(|operation| {
            (!operation.runtime_leaf.is_empty()).then_some(operation.runtime_leaf.as_str())
        })
        .collect::<std::collections::BTreeSet<_>>();
    for leaf in leaves {
        rows.push(stage1_json_object([
            ("deterministic", JsonValue::Bool(true)),
            ("identity", stage1_json_string(leaf)),
            ("rowKind", stage1_json_string("runtime-leaf")),
            ("schema", stage1_json_string(topaz_kernel::LOWERED_SCHEMA)),
        ]));
    }
    rows.push(stage1_json_object([
        ("identity", stage1_json_string("stage1-ir-table/v1")),
        ("rowKind", stage1_json_string("runtime-template")),
        ("schema", stage1_json_string(topaz_kernel::LOWERED_SCHEMA)),
        (
            "sha256",
            stage1_json_string(FIXED_POINT_RUNTIME_TEMPLATE_SHA256),
        ),
    ]));
    for operation in &result.operations {
        let source_id = sources.get(&operation.module).ok_or_else(|| {
            format!(
                "Stage 1 operation references unknown module `{}`",
                operation.module
            )
        })?;
        let binding = if operation.binding_name.is_empty() {
            JsonValue::Null
        } else {
            stage1_json_object([
                (
                    "declarationIdentity",
                    if operation.declaration_identity.is_empty() {
                        JsonValue::Null
                    } else {
                        stage1_json_string(&operation.declaration_identity)
                    },
                ),
                ("mutable", JsonValue::Bool(operation.binding_mutable)),
                ("name", stage1_json_string(&operation.binding_name)),
                ("storage", stage1_json_string(&operation.binding_storage)),
            ])
        };
        let control = if operation.control_kind.is_empty() {
            JsonValue::Null
        } else {
            stage1_json_object([
                (
                    "cleanupIds",
                    stage1_json_array(
                        operation
                            .cleanup_ids
                            .iter()
                            .map(|value| stage1_json_string(value))
                            .collect(),
                    ),
                ),
                ("kind", stage1_json_string(&operation.control_kind)),
                (
                    "target",
                    if operation.control_target.is_empty() {
                        JsonValue::Null
                    } else {
                        stage1_json_string(&operation.control_target)
                    },
                ),
            ])
        };
        let (kind, detail) = if let Some(kind) = operation.kind.strip_prefix("expression/") {
            (
                "expression",
                stage1_json_object([
                    (
                        "detail",
                        if operation.detail.is_empty() {
                            JsonValue::Null
                        } else {
                            stage1_json_string(&operation.detail)
                        },
                    ),
                    ("expressionKind", stage1_json_string(kind)),
                ]),
            )
        } else if let Some(kind) = operation.kind.strip_prefix("pattern/") {
            (
                "pattern",
                stage1_json_object([
                    (
                        "detail",
                        if operation.detail.is_empty() {
                            JsonValue::Null
                        } else {
                            stage1_json_string(&operation.detail)
                        },
                    ),
                    ("patternKind", stage1_json_string(kind)),
                ]),
            )
        } else {
            (operation.kind.as_str(), JsonValue::Null)
        };
        rows.push(stage1_json_object([
            ("binding", binding),
            ("call", stage1_lowered_call_plan(operation, source_id)?),
            ("control", control),
            ("detail", detail),
            ("kind", stage1_json_string(kind)),
            ("module", stage1_json_string(&operation.module)),
            (
                "operands",
                stage1_json_array(
                    operation
                        .operands
                        .iter()
                        .map(|value| stage1_json_string(value))
                        .collect(),
                ),
            ),
            ("operationId", stage1_json_string(&operation.id)),
            (
                "parentOperationId",
                operation
                    .parent_id
                    .as_deref()
                    .map_or(JsonValue::Null, stage1_json_string),
            ),
            (
                "representation",
                if operation.representation.is_empty() {
                    JsonValue::Null
                } else {
                    stage1_json_string(&operation.representation)
                },
            ),
            ("role", stage1_json_string(&operation.role)),
            ("rowKind", stage1_json_string("operation")),
            (
                "runtimeLeaf",
                if operation.runtime_leaf.is_empty() {
                    JsonValue::Null
                } else {
                    stage1_json_string(&operation.runtime_leaf)
                },
            ),
            ("schema", stage1_json_string(topaz_kernel::LOWERED_SCHEMA)),
            ("semanticType", JsonValue::Null),
            (
                "span",
                stage1_json_object([
                    ("hi", stage1_json_number(operation.hi.into())),
                    ("lo", stage1_json_number(operation.lo.into())),
                    ("sourceId", stage1_json_string(source_id)),
                ]),
            ),
        ]));
    }
    let mut output = String::new();
    for row in rows {
        output.push_str(&stage1_encode_json(&row));
        output.push('\n');
    }
    Ok(output.into_bytes())
}
