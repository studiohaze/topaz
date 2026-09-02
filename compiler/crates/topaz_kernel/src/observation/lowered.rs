use super::bundle::*;
use super::typed::*;
use super::*;

fn lowered_kind(value: &topaz_hir::LoweredOperationKind) -> (&'static str, JsonValue) {
    use topaz_hir::{
        LoweredExpressionKind as E, LoweredOperationKind as O, LoweredPatternKind as P,
    };
    match value {
        O::Module => ("module", JsonValue::Null),
        O::Import => ("import", JsonValue::Null),
        O::Export => ("export", JsonValue::Null),
        O::Function => ("function", JsonValue::Null),
        O::TypeAlias => ("type-alias", JsonValue::Null),
        O::Enum => ("enum", JsonValue::Null),
        O::Record => ("record", JsonValue::Null),
        O::Newtype => ("newtype", JsonValue::Null),
        O::Protocol => ("protocol", JsonValue::Null),
        O::Implementation => ("implementation", JsonValue::Null),
        O::Let => ("let", JsonValue::Null),
        O::Constant => ("constant", JsonValue::Null),
        O::Assignment => ("assignment", JsonValue::Null),
        O::Return => ("return", JsonValue::Null),
        O::Defer => ("defer", JsonValue::Null),
        O::Using => ("using", JsonValue::Null),
        O::While => ("while", JsonValue::Null),
        O::Break => ("break", JsonValue::Null),
        O::Continue => ("continue", JsonValue::Null),
        O::Expression(value) => {
            let (kind, detail) = match value {
                E::Integer { spelling } => ("integer", object([("spelling", string(spelling))])),
                E::Float { spelling } => ("float", object([("spelling", string(spelling))])),
                E::Duration { spelling } => ("duration", object([("spelling", string(spelling))])),
                E::Boolean(value) => ("boolean", object([("value", boolean(*value))])),
                E::Null => ("null", JsonValue::Null),
                E::Unit => ("unit", JsonValue::Null),
                E::String { tag, multiline } => (
                    "string",
                    object([
                        ("tag", tag.as_ref().map_or(JsonValue::Null, string)),
                        ("multiline", boolean(*multiline)),
                    ]),
                ),
                E::Identifier { name, target } => (
                    "identifier",
                    object([
                        ("name", string(name)),
                        ("target", target.as_ref().map_or(JsonValue::Null, string)),
                    ]),
                ),
                E::Placeholder => ("placeholder", JsonValue::Null),
                E::Parenthesized => ("parenthesized", JsonValue::Null),
                E::Block => ("block", JsonValue::Null),
                E::If => ("if", JsonValue::Null),
                E::Match => ("match", JsonValue::Null),
                E::For => ("for", JsonValue::Null),
                E::Loop => ("loop", JsonValue::Null),
                E::Concurrent => ("concurrent", JsonValue::Null),
                E::Call => ("call", JsonValue::Null),
                E::Member { name, target } => (
                    "member",
                    object([
                        ("name", string(name)),
                        ("target", target.as_ref().map_or(JsonValue::Null, string)),
                    ]),
                ),
                E::Index => ("index", JsonValue::Null),
                E::OptionalMember { name, target } => (
                    "optional-member",
                    object([
                        ("name", string(name)),
                        ("target", target.as_ref().map_or(JsonValue::Null, string)),
                    ]),
                ),
                E::ResultPropagation => ("result-propagation", JsonValue::Null),
                E::Unary { operator } => ("unary", object([("operator", string(operator))])),
                E::Binary { operator } => ("binary", object([("operator", string(operator))])),
                E::Range { inclusive } => ("range", object([("inclusive", boolean(*inclusive))])),
                E::Compose => ("compose", JsonValue::Null),
                E::Pipeline => ("pipeline", JsonValue::Null),
                E::Lambda => ("lambda", JsonValue::Null),
                E::RecordLiteral => ("record-literal", JsonValue::Null),
                E::RecordUpdate => ("record-update", JsonValue::Null),
                E::Array => ("array", JsonValue::Null),
                E::Set => ("set", JsonValue::Null),
                E::Map => ("map", JsonValue::Null),
                E::Comprehension { collection } => (
                    "comprehension",
                    object([("collection", string(collection))]),
                ),
                E::StringText { text } => ("string-text", object([("text", string(text))])),
            };
            (
                "expression",
                object([("detail", detail), ("expressionKind", string(kind))]),
            )
        }
        O::Pattern(value) => {
            let (kind, detail) = match value {
                P::Alternatives => ("alternatives", JsonValue::Null),
                P::Wildcard => ("wildcard", JsonValue::Null),
                P::Literal => ("literal", JsonValue::Null),
                P::Range { inclusive } => ("range", object([("inclusive", boolean(*inclusive))])),
                P::Binding { name } => ("binding", object([("name", string(name))])),
                P::TypedBinding { name } => ("typed-binding", object([("name", string(name))])),
                P::Constructor { name } => ("constructor", object([("name", string(name))])),
                P::List => ("list", JsonValue::Null),
                P::Record => ("record", JsonValue::Null),
                P::NominalRecord { name } => ("nominal-record", object([("name", string(name))])),
                P::Rest => ("rest", JsonValue::Null),
            };
            (
                "pattern",
                object([("detail", detail), ("patternKind", string(kind))]),
            )
        }
    }
}

pub(super) fn lowered_rows(
    unit: &KernelUnit,
    sources: &BTreeMap<u32, SourceIdentity>,
) -> Vec<JsonValue> {
    let Some(lowered) = &unit.lowered else {
        return Vec::new();
    };
    let source = |file: u32| {
        sources
            .get(&file)
            .map(|source| source.source_id.as_str())
            .unwrap_or("unknown-source")
    };
    let mut rows = Vec::new();
    for module in &lowered.modules {
        rows.push(object([
            ("entry", boolean(module.is_entry)),
            ("extern", boolean(module.is_extern)),
            ("identity", string(&module.identity)),
            (
                "initializationOrdinal",
                unsigned(module.initialization_ordinal as u64),
            ),
            (
                "operationIds",
                array(module.operation_ids.iter().map(string)),
            ),
            ("path", string(&module.path)),
            ("rowKind", string("module")),
            ("schema", string(LOWERED_SCHEMA)),
            ("sourceId", string(source(module.file.0))),
        ]));
    }
    for leaf in &lowered.runtime.leaves {
        rows.push(object([
            ("deterministic", boolean(leaf.deterministic)),
            ("identity", string(&leaf.identity)),
            ("rowKind", string("runtime-leaf")),
            ("schema", string(LOWERED_SCHEMA)),
        ]));
    }
    for template in &lowered.runtime.templates {
        rows.push(object([
            ("identity", string(&template.identity)),
            ("rowKind", string("runtime-template")),
            ("schema", string(LOWERED_SCHEMA)),
            ("sha256", string(&template.sha256)),
        ]));
    }
    for operation in &lowered.operations {
        let (kind, detail) = lowered_kind(&operation.kind);
        let binding = operation
            .binding
            .as_ref()
            .map_or(JsonValue::Null, |binding| {
                object([
                    (
                        "declarationIdentity",
                        binding
                            .declaration_identity
                            .as_ref()
                            .map_or(JsonValue::Null, string),
                    ),
                    ("mutable", boolean(binding.mutable)),
                    ("name", string(&binding.name)),
                    (
                        "storage",
                        string(match binding.storage {
                            topaz_hir::LoweredStorage::Local => "local",
                            topaz_hir::LoweredStorage::Module => "module",
                            topaz_hir::LoweredStorage::Captured => "captured",
                            topaz_hir::LoweredStorage::Parameter => "parameter",
                            topaz_hir::LoweredStorage::Temporary => "temporary",
                        }),
                    ),
                ])
            });
        let control = operation
            .control
            .as_ref()
            .map_or(JsonValue::Null, |control| {
                object([
                    ("cleanupIds", array(control.cleanup_ids.iter().map(string))),
                    (
                        "kind",
                        string(match control.kind {
                            topaz_hir::LoweredControlKind::Branch => "branch",
                            topaz_hir::LoweredControlKind::Match => "match",
                            topaz_hir::LoweredControlKind::Loop => "loop",
                            topaz_hir::LoweredControlKind::Break => "break",
                            topaz_hir::LoweredControlKind::Continue => "continue",
                            topaz_hir::LoweredControlKind::Return => "return",
                            topaz_hir::LoweredControlKind::Cleanup => "cleanup",
                            topaz_hir::LoweredControlKind::Propagate => "propagate",
                            topaz_hir::LoweredControlKind::Concurrent => "concurrent",
                        }),
                    ),
                    (
                        "target",
                        control.target.as_ref().map_or(JsonValue::Null, string),
                    ),
                ])
            });
        rows.push(object([
            ("binding", binding),
            (
                "call",
                operation
                    .call
                    .as_ref()
                    .map_or(JsonValue::Null, |call| typed_call_plan(call, sources)),
            ),
            ("control", control),
            ("detail", detail),
            ("kind", string(kind)),
            ("module", string(&operation.module)),
            ("operationId", string(&operation.id)),
            ("operands", array(operation.operands.iter().map(string))),
            (
                "parentOperationId",
                operation.parent.as_ref().map_or(JsonValue::Null, string),
            ),
            (
                "representation",
                operation
                    .representation
                    .map_or(JsonValue::Null, |value| string(value.name())),
            ),
            (
                "role",
                string(match operation.role {
                    topaz_hir::LoweredRole::ModuleInitialization => "module-initialization",
                    topaz_hir::LoweredRole::Statement => "statement",
                    topaz_hir::LoweredRole::Expression => "expression",
                    topaz_hir::LoweredRole::Pattern => "pattern",
                    topaz_hir::LoweredRole::Binding => "binding",
                    topaz_hir::LoweredRole::Declaration => "declaration",
                    topaz_hir::LoweredRole::Cleanup => "cleanup",
                }),
            ),
            ("rowKind", string("operation")),
            (
                "runtimeLeaf",
                operation
                    .runtime_leaf
                    .as_ref()
                    .map_or(JsonValue::Null, string),
            ),
            ("schema", string(LOWERED_SCHEMA)),
            (
                "semanticType",
                operation
                    .semantic_type
                    .as_ref()
                    .map_or(JsonValue::Null, semantic_type),
            ),
            ("span", span(source(operation.span.file.0), operation.span)),
        ]));
    }
    rows
}

/// Adds lowered, generated-source, product, and fixed-point members to a preview.
pub fn complete_compiler_preview_observation(
    typed: ObservationBundle,
    request: &KernelRequest,
    completion: CompilerPreviewCompletion<'_>,
) -> Result<ObservationBundle, String> {
    let CompilerPreviewCompletion {
        lowered_jsonl,
        generated_rust,
        product,
        runtime_template_identity,
        runtime_template_sha256,
        producer_stage,
        fixed_point,
    } = completion;
    if request.terminal_phase() != crate::TerminalPhase::RustSource {
        return Err("compiler producer observation requires the rust-source terminal".to_string());
    }
    let (product_path, product_schema, provenance) = match producer_stage {
        1 => (
            "stage1-product.json",
            STAGE1_PRODUCT_SCHEMA,
            crate::BootstrapProvenance::topaz_stage1_preview(),
        ),
        2 => (
            "stage2-product.json",
            STAGE2_PRODUCT_SCHEMA,
            if fixed_point.is_some() {
                crate::BootstrapProvenance::topaz_stage2_fixed_point()
            } else {
                crate::BootstrapProvenance::topaz_stage2_preview()
            },
        ),
        _ => {
            return Err(format!(
                "unsupported compiler producer stage {producer_stage}"
            ));
        }
    };
    let typed_request_bytes = typed
        .files
        .iter()
        .find(|file| file.path == "request.json")
        .map(|file| file.bytes.clone())
        .ok_or_else(|| "typed compiler producer base omitted request.json".to_string())?;
    let mut files = typed
        .files
        .into_iter()
        .filter(|file| {
            !matches!(
                file.path.as_str(),
                "topaz-observation.json" | "response.json" | "request.json" | "provenance.json"
            )
        })
        .map(|file| (file.path, (file.schema, file.bytes)))
        .collect::<BTreeMap<_, _>>();
    files.insert(
        "lowered.jsonl".to_string(),
        (LOWERED_SCHEMA.to_string(), lowered_jsonl),
    );
    let rust_rows = vec![object([
        ("byteLength", unsigned(generated_rust.len() as u64)),
        ("rowKind", string("generated-source")),
        ("schema", string(RUST_SOURCE_SCHEMA)),
        ("sha256", string(sha256(generated_rust.as_bytes()))),
        ("source", string(generated_rust)),
    ])];
    files.insert(
        "rust-source.jsonl".to_string(),
        (RUST_SOURCE_SCHEMA.to_string(), encode_jsonl(&rust_rows)),
    );
    files.insert(
        product_path.to_string(),
        (product_schema.to_string(), product),
    );
    if let Some(fixed_point) = fixed_point {
        if producer_stage != 2 {
            return Err("fixed-point record requires the Stage 2 producer".to_string());
        }
        files.insert(
            "stage2-fixed-point.json".to_string(),
            (STAGE2_FIXED_POINT_SCHEMA.to_string(), fixed_point),
        );
    }
    files.insert(
        "provenance.json".to_string(),
        (
            crate::PROVENANCE_SCHEMA.to_string(),
            encode(&provenance_json_with(
                &provenance,
                vec![object([
                    ("identity", string(runtime_template_identity)),
                    ("sha256", string(runtime_template_sha256)),
                ])],
            )),
        ),
    );

    let mut request_values = crate::canonical::validate(&typed_request_bytes, false)?;
    if request_values.len() != 1 {
        return Err("typed compiler producer request projection has multiple values".to_string());
    }
    let JsonValue::Object(request_fields) = request_values.remove(0) else {
        return Err("typed compiler producer request projection is not an object".to_string());
    };
    let mut request_fields = request_fields.as_ref().clone();
    request_fields.insert("terminalPhase".into(), string("rust-source"));
    let request_bytes = encode(&JsonValue::Object(request_fields.into()));
    let request_digest = {
        let mut material = request_bytes.clone();
        for (path, (_, bytes)) in files
            .iter()
            .filter(|(path, _)| path.starts_with("sources/"))
        {
            material.extend_from_slice(path.as_bytes());
            material.push(0);
            material.extend_from_slice(bytes);
        }
        sha256(&material)
    };
    files.insert(
        "request.json".to_string(),
        (crate::REQUEST_SCHEMA.to_string(), request_bytes),
    );
    files.insert(
        "response.json".to_string(),
        (
            crate::RESPONSE_SCHEMA.to_string(),
            encode(&object([
                ("highestCompletedPhase", string("rust-source")),
                (
                    "phases",
                    object([
                        ("ast", string("produced")),
                        ("lowered", string("produced")),
                        ("resolved", string("produced")),
                        ("rustSource", string("produced")),
                        ("tokens", string("produced")),
                        ("typed", string("produced")),
                    ]),
                ),
                (
                    "projectionDigests",
                    object([
                        ("ast", string(sha256(&files["ast.jsonl"].1))),
                        ("diagnostics", string(sha256(&files["diagnostics.jsonl"].1))),
                        ("lowered", string(sha256(&files["lowered.jsonl"].1))),
                        ("resolved", string(sha256(&files["resolved.jsonl"].1))),
                        ("rustSource", string(sha256(&files["rust-source.jsonl"].1))),
                        ("sourceSet", string(sha256(&files["source-set.jsonl"].1))),
                        ("tokens", string(sha256(&files["tokens.jsonl"].1))),
                        ("typed", string(sha256(&files["typed.jsonl"].1))),
                    ]),
                ),
                ("requestDigest", string(request_digest)),
                ("schema", string(crate::RESPONSE_SCHEMA)),
                ("status", string("completed")),
            ])),
        ),
    );
    finish_observation(files, request.budgets().max_projection_bytes, true)
}
