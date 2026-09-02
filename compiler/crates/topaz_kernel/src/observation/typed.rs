use super::bundle::*;
use super::resolved::*;
use super::*;

pub(super) fn semantic_type(value: &topaz_hir::SemanticType) -> JsonValue {
    use topaz_hir::SemanticType as T;
    match value {
        T::Primitive(value) => object([
            ("kind", string("primitive")),
            (
                "name",
                string(match value {
                    topaz_hir::SemanticPrimitive::Int => "int",
                    topaz_hir::SemanticPrimitive::Float => "float",
                    topaz_hir::SemanticPrimitive::String => "string",
                    topaz_hir::SemanticPrimitive::Bool => "bool",
                    topaz_hir::SemanticPrimitive::Unit => "unit",
                }),
            ),
        ]),
        T::Literal(value) => {
            use topaz_hir::SemanticLiteral as L;
            let (name, value) = match value {
                L::String(value) => ("string", string(value)),
                L::Int(value) => ("int", string(value.to_string())),
                L::Float(value) => ("float", string(value)),
                L::Bool(value) => ("bool", boolean(*value)),
                L::Null => ("null", JsonValue::Null),
            };
            object([
                ("kind", string("literal")),
                ("name", string(name)),
                ("value", value),
            ])
        }
        T::Union(values) => object([
            ("kind", string("union")),
            ("members", array(values.iter().map(semantic_type))),
        ]),
        T::Record(fields) => object([
            (
                "fields",
                array(fields.iter().map(|field| {
                    object([
                        ("name", string(&field.name)),
                        ("type", semantic_type(&field.ty)),
                    ])
                })),
            ),
            ("kind", string("record")),
        ]),
        T::Constructor {
            constructor,
            arguments,
        } => object([
            ("arguments", array(arguments.iter().map(semantic_type))),
            (
                "constructor",
                string(match constructor {
                    topaz_hir::SemanticConstructor::Array => "Array",
                    topaz_hir::SemanticConstructor::Map => "Map",
                    topaz_hir::SemanticConstructor::Set => "Set",
                    topaz_hir::SemanticConstructor::Option => "Option",
                    topaz_hir::SemanticConstructor::Result => "Result",
                    topaz_hir::SemanticConstructor::Range => "Range",
                }),
            ),
            ("kind", string("constructor")),
        ]),
        T::Function {
            parameters,
            variadic,
            result,
        } => object([
            ("kind", string("function")),
            ("parameters", array(parameters.iter().map(semantic_type))),
            ("result", semantic_type(result)),
            (
                "variadic",
                variadic.as_deref().map_or(JsonValue::Null, semantic_type),
            ),
        ]),
        T::Foreign {
            identity,
            arguments,
        } => object([
            ("arguments", array(arguments.iter().map(semantic_type))),
            ("identity", string(identity)),
            ("kind", string("foreign")),
        ]),
        T::Rigid { name, origin } => object([
            ("kind", string("rigid")),
            ("name", string(name)),
            ("origin", string(origin)),
        ]),
        T::Enum {
            identity,
            arguments,
        }
        | T::NominalRecord {
            identity,
            arguments,
        }
        | T::Newtype {
            identity,
            arguments,
        } => object([
            ("arguments", array(arguments.iter().map(semantic_type))),
            ("identity", string(identity)),
            (
                "kind",
                string(match value {
                    T::Enum { .. } => "enum",
                    T::NominalRecord { .. } => "nominal-record",
                    T::Newtype { .. } => "newtype",
                    _ => unreachable!(),
                }),
            ),
        ]),
        T::InferenceVariable => object([("kind", string("inference-variable"))]),
        other => object([(
            "kind",
            string(match other {
                T::Template => "template",
                T::File => "file",
                T::JsonValue => "json-value",
                T::Bytes => "bytes",
                T::ByteBuffer => "byte-buffer",
                T::Path => "path",
                T::Regex => "regex",
                T::Match => "match",
                T::TomlValue => "toml-value",
                T::Url => "url",
                T::Date => "date",
                T::BigInt => "big-int",
                T::Decimal => "decimal",
                T::RoundingMode => "rounding-mode",
                T::Unknown => "unknown",
                _ => unreachable!(),
            }),
        )]),
    }
}

fn flatten_semantic_type(
    rows: &mut Vec<JsonValue>,
    value: &topaz_hir::SemanticType,
    parent: i64,
    field: &str,
    index: usize,
    edge_name: &str,
) {
    use topaz_hir::SemanticType as T;
    let (kind, name, literal, identity, origin) = match value {
        T::Primitive(value) => (
            "primitive",
            match value {
                topaz_hir::SemanticPrimitive::Int => "int",
                topaz_hir::SemanticPrimitive::Float => "float",
                topaz_hir::SemanticPrimitive::String => "string",
                topaz_hir::SemanticPrimitive::Bool => "bool",
                topaz_hir::SemanticPrimitive::Unit => "unit",
            },
            String::new(),
            "",
            "",
        ),
        T::Literal(value) => {
            use topaz_hir::SemanticLiteral as L;
            let (name, literal) = match value {
                L::String(value) => ("string", value.clone()),
                L::Int(value) => ("int", value.to_string()),
                L::Float(value) => ("float", value.clone()),
                L::Bool(value) => ("bool", value.to_string()),
                L::Null => ("null", String::new()),
            };
            ("literal", name, literal, "", "")
        }
        T::Union(_) => ("union", "", String::new(), "", ""),
        T::Record(_) => ("record", "", String::new(), "", ""),
        T::Constructor { constructor, .. } => (
            "constructor",
            match constructor {
                topaz_hir::SemanticConstructor::Array => "Array",
                topaz_hir::SemanticConstructor::Map => "Map",
                topaz_hir::SemanticConstructor::Set => "Set",
                topaz_hir::SemanticConstructor::Option => "Option",
                topaz_hir::SemanticConstructor::Result => "Result",
                topaz_hir::SemanticConstructor::Range => "Range",
            },
            String::new(),
            "",
            "",
        ),
        T::Function { .. } => ("function", "", String::new(), "", ""),
        T::Foreign { identity, .. } => ("foreign", "", String::new(), identity.as_str(), ""),
        T::Rigid { name, origin } => ("rigid", name.as_str(), String::new(), "", origin.as_str()),
        T::Enum { identity, .. } => ("enum", "", String::new(), identity.as_str(), ""),
        T::NominalRecord { identity, .. } => {
            ("nominal-record", "", String::new(), identity.as_str(), "")
        }
        T::Newtype { identity, .. } => ("newtype", "", String::new(), identity.as_str(), ""),
        T::Template => ("template", "", String::new(), "", ""),
        T::File => ("file", "", String::new(), "", ""),
        T::JsonValue => ("json-value", "", String::new(), "", ""),
        T::Bytes => ("bytes", "", String::new(), "", ""),
        T::ByteBuffer => ("byte-buffer", "", String::new(), "", ""),
        T::Path => ("path", "", String::new(), "", ""),
        T::Regex => ("regex", "", String::new(), "", ""),
        T::Match => ("match", "", String::new(), "", ""),
        T::TomlValue => ("toml-value", "", String::new(), "", ""),
        T::Url => ("url", "", String::new(), "", ""),
        T::Date => ("date", "", String::new(), "", ""),
        T::BigInt => ("big-int", "", String::new(), "", ""),
        T::Decimal => ("decimal", "", String::new(), "", ""),
        T::RoundingMode => ("rounding-mode", "", String::new(), "", ""),
        T::Unknown => ("unknown", "", String::new(), "", ""),
        T::InferenceVariable => ("inference-variable", "", String::new(), "", ""),
    };
    let ordinal = rows.len();
    rows.push(object([
        ("edgeName", string(edge_name)),
        ("field", string(field)),
        ("identity", string(identity)),
        ("index", unsigned(index as u64)),
        ("kind", string(kind)),
        ("name", string(name)),
        ("origin", string(origin)),
        ("parent", signed(parent)),
        ("value", string(literal)),
    ]));
    let mut add_children = |values: &[topaz_hir::SemanticType], field: &str| {
        for (index, child) in values.iter().enumerate() {
            flatten_semantic_type(rows, child, ordinal as i64, field, index, "");
        }
    };
    match value {
        T::Union(values) => add_children(values, "members"),
        T::Record(fields) => {
            for (index, child) in fields.iter().enumerate() {
                flatten_semantic_type(
                    rows,
                    &child.ty,
                    ordinal as i64,
                    "fields",
                    index,
                    &child.name,
                );
            }
        }
        T::Constructor { arguments, .. }
        | T::Foreign { arguments, .. }
        | T::Enum { arguments, .. }
        | T::NominalRecord { arguments, .. }
        | T::Newtype { arguments, .. } => add_children(arguments, "arguments"),
        T::Function {
            parameters,
            variadic,
            result,
        } => {
            add_children(parameters, "parameters");
            if let Some(variadic) = variadic {
                flatten_semantic_type(rows, variadic, ordinal as i64, "variadic", 0, "");
            }
            flatten_semantic_type(rows, result, ordinal as i64, "result", 0, "");
        }
        _ => {}
    }
}

/// Canonical flat semantic type tree used inside private target-runtime facts.
pub fn semantic_type_atoms_json(value: &topaz_hir::SemanticType) -> JsonValue {
    let mut rows = Vec::new();
    flatten_semantic_type(&mut rows, value, -1, "root", 0, "");
    array(rows)
}

pub(super) fn typed_call_plan(
    value: &topaz_hir::CallPlan,
    sources: &BTreeMap<u32, SourceIdentity>,
) -> JsonValue {
    let source = |value: Span| {
        sources
            .get(&value.file.0)
            .map(|source| source.source_id.as_str())
            .unwrap_or("unknown-source")
    };
    let callee = match &value.callee {
        topaz_hir::CalleePlan::Value => object([("kind", string("value"))]),
        topaz_hir::CalleePlan::Member {
            method,
            class,
            optional,
            shadow_first,
        } => object([
            (
                "class",
                string(match class {
                    topaz_hir::MethodClass::Hof => "higher-order",
                    topaz_hir::MethodClass::LazyCallback => "lazy-callback",
                    topaz_hir::MethodClass::Mutator => "mutator",
                    topaz_hir::MethodClass::Resource => "resource",
                    topaz_hir::MethodClass::Other => "other",
                }),
            ),
            ("kind", string("member")),
            ("method", string(method)),
            ("optional", boolean(*optional)),
            ("shadowFirst", boolean(*shadow_first)),
        ]),
        topaz_hir::CalleePlan::Pipe { stage_method } => object([
            ("kind", string("pipe")),
            (
                "stageMethod",
                stage_method.as_ref().map_or(JsonValue::Null, string),
            ),
        ]),
    };
    let arguments = value.args.iter().map(|argument| {
        let binding = match &argument.binding {
            topaz_hir::ArgBinding::Positional => object([("kind", string("positional"))]),
            topaz_hir::ArgBinding::Named(name) => {
                object([("kind", string("named")), ("name", string(name))])
            }
            topaz_hir::ArgBinding::Spread => object([("kind", string("spread"))]),
            topaz_hir::ArgBinding::InsertedLead => object([("kind", string("inserted-lead"))]),
        };
        object([
            ("binding", binding),
            (
                "sourceIndex",
                argument
                    .source_index
                    .map_or(JsonValue::Null, |index| unsigned(index as u64)),
            ),
            ("span", span(source(argument.span), argument.span)),
        ])
    });
    let evaluation = value.eval.iter().map(|step| match step {
        topaz_hir::EvalStep::Callee => object([("kind", string("callee"))]),
        topaz_hir::EvalStep::Receiver => object([("kind", string("receiver"))]),
        topaz_hir::EvalStep::OptionalGuard => object([("kind", string("optional-guard"))]),
        topaz_hir::EvalStep::PipeLead => object([("kind", string("pipe-lead"))]),
        topaz_hir::EvalStep::Arg(index) => object([
            ("argumentIndex", unsigned(*index as u64)),
            ("kind", string("argument")),
        ]),
    });
    object([
        ("arguments", array(arguments)),
        ("binding", object([("kind", string("runtime"))])),
        ("callee", callee),
        ("evaluation", array(evaluation)),
    ])
}

pub(super) fn typed_rows(
    unit: &KernelUnit,
    sources: &BTreeMap<u32, SourceIdentity>,
    identity_nodes: &BTreeMap<(u32, u32, u32), String>,
) -> Vec<JsonValue> {
    let Some(typed) = unit
        .checked
        .as_ref()
        .and_then(|checked| checked.typed_hir.as_ref())
    else {
        return Vec::new();
    };
    let node_id = |span: Span| {
        identity_nodes
            .get(&(span.file.0, span.lo, span.hi))
            .cloned()
            .map_or(JsonValue::Null, string)
    };
    let source = |span: Span| {
        sources
            .get(&span.file.0)
            .map(|source| source.source_id.as_str())
            .unwrap_or("unknown-source")
    };
    let mut rows = Vec::new();
    for fact in &typed.nodes {
        rows.push(object([
            ("ambient", boolean(fact.ambient)),
            (
                "nodeKind",
                string(match fact.kind {
                    topaz_hir::TypedNodeKind::Expression => "expression",
                    topaz_hir::TypedNodeKind::Pattern => "pattern",
                    topaz_hir::TypedNodeKind::Binding => "binding",
                    topaz_hir::TypedNodeKind::Declaration => "declaration",
                    topaz_hir::TypedNodeKind::Type => "type",
                }),
            ),
            ("nodeId", node_id(fact.span)),
            ("rowKind", string("node")),
            ("schema", string(TYPED_SCHEMA)),
            ("sourceId", string(source(fact.span))),
            ("span", span(source(fact.span), fact.span)),
            ("type", semantic_type(&fact.ty)),
        ]));
    }
    for fact in &typed.calls {
        let ambient = fact.callee_type.has_hole() || fact.result_type.has_hole();
        rows.push(object([
            ("ambient", boolean(ambient)),
            ("callNodeId", node_id(fact.span)),
            ("calleeNodeId", node_id(fact.callee_span)),
            (
                "calleeSpan",
                span(source(fact.callee_span), fact.callee_span),
            ),
            ("calleeType", semantic_type(&fact.callee_type)),
            ("plan", typed_call_plan(&fact.plan, sources)),
            ("resultType", semantic_type(&fact.result_type)),
            ("rowKind", string("call")),
            ("schema", string(TYPED_SCHEMA)),
            ("sourceId", string(source(fact.span))),
            ("span", span(source(fact.span), fact.span)),
            (
                "targetIdentity",
                fact.target_identity
                    .as_ref()
                    .map_or(JsonValue::Null, string),
            ),
        ]));
    }
    for fact in &typed.captures {
        rows.push(object([
            ("ambient", boolean(fact.ambient)),
            ("closureNodeId", node_id(fact.closure_span)),
            ("declarationNodeId", node_id(fact.declaration_span)),
            ("name", string(&fact.name)),
            ("referenceNodeId", node_id(fact.reference_span)),
            ("rowKind", string("capture")),
            ("schema", string(TYPED_SCHEMA)),
            ("sourceId", string(source(fact.reference_span))),
            (
                "span",
                span(source(fact.reference_span), fact.reference_span),
            ),
            ("type", semantic_type(&fact.ty)),
        ]));
    }
    rows
}

fn preview_typed_bytes(
    modules: &[CanonicalPreviewModule],
    node_ordinals: &[PreviewNodeOrdinalMap],
    nodes: &[topaz_hir::TypedNode],
    calls: &[topaz_hir::TypedCall],
    captures: &[topaz_hir::TypedCapture],
) -> Result<Vec<u8>, String> {
    fn contains_inference_variable(ty: &topaz_hir::SemanticType) -> bool {
        use topaz_hir::SemanticType as T;

        match ty {
            T::InferenceVariable => true,
            T::Union(values) => values.iter().any(contains_inference_variable),
            T::Record(fields) => fields
                .iter()
                .any(|field| contains_inference_variable(&field.ty)),
            T::Constructor { arguments, .. }
            | T::Foreign { arguments, .. }
            | T::Enum { arguments, .. }
            | T::NominalRecord { arguments, .. }
            | T::Newtype { arguments, .. } => arguments.iter().any(contains_inference_variable),
            T::Function {
                parameters,
                variadic,
                result,
            } => {
                parameters.iter().any(contains_inference_variable)
                    || variadic.as_deref().is_some_and(contains_inference_variable)
                    || contains_inference_variable(result)
            }
            T::Primitive(_)
            | T::Literal(_)
            | T::Rigid { .. }
            | T::Template
            | T::File
            | T::JsonValue
            | T::Bytes
            | T::ByteBuffer
            | T::Path
            | T::Regex
            | T::Match
            | T::TomlValue
            | T::Url
            | T::Date
            | T::BigInt
            | T::Decimal
            | T::RoundingMode
            | T::Unknown => false,
        }
    }

    let mut source_order = (0..modules.len()).collect::<Vec<_>>();
    source_order.sort_by_key(|index| (&modules[*index].identity, &modules[*index].path));
    let mut sources = BTreeMap::<u32, SourceIdentity>::new();
    for (ordinal, index) in source_order.iter().copied().enumerate() {
        let module = &modules[index];
        sources.insert(
            u32::try_from(index)
                .map_err(|_| "typed preview module index exceeds u32".to_string())?,
            SourceIdentity {
                source_id: source_id(&module.identity, &module.path),
                module: module.identity.clone(),
                path: topaz_resolve::normalize_path(&module.path),
                ordinal: ordinal as u64,
            },
        );
    }
    let node_id = |value: Span| -> JsonValue {
        node_ordinals
            .get(value.file.0 as usize)
            .and_then(|ordinals| ordinals.get(&(value.lo, value.hi)))
            .and_then(|ordinal| {
                sources
                    .get(&value.file.0)
                    .map(|source| format!("{}#n{ordinal:08x}", source.source_id))
            })
            .map_or(JsonValue::Null, string)
    };
    let source = |value: Span| {
        sources
            .get(&value.file.0)
            .map(|source| source.source_id.as_str())
            .unwrap_or("unknown-source")
    };
    let mut inference_failures = Vec::new();
    for fact in nodes {
        if contains_inference_variable(&fact.ty) {
            inference_failures.push(format!(
                "node {}:{}..{} ({:?})",
                fact.module, fact.span.lo, fact.span.hi, fact.ty
            ));
        }
    }
    for fact in calls {
        if contains_inference_variable(&fact.callee_type)
            || contains_inference_variable(&fact.result_type)
        {
            inference_failures.push(format!(
                "call {}:{}..{} (callee {:?}, result {:?})",
                fact.module, fact.span.lo, fact.span.hi, fact.callee_type, fact.result_type
            ));
        }
    }
    for fact in captures {
        if contains_inference_variable(&fact.ty) {
            inference_failures.push(format!(
                "capture {}:{}..{} ({:?})",
                fact.module, fact.reference_span.lo, fact.reference_span.hi, fact.ty
            ));
        }
    }
    if !inference_failures.is_empty() {
        inference_failures.sort();
        let total = inference_failures.len();
        inference_failures.truncate(32);
        return Err(format!(
            "typed preview retained {total} inference-local variable facts; first failures: {}",
            inference_failures.join("; ")
        ));
    }
    let mut bytes = Vec::new();
    let mut ordered_nodes = nodes.to_vec();
    ordered_nodes.sort_by_key(|fact| {
        (
            fact.module.clone(),
            fact.span.file.0,
            fact.span.lo,
            fact.span.hi,
            fact.kind,
        )
    });
    ordered_nodes.dedup_by(|left, right| {
        left.module == right.module && left.kind == right.kind && left.span == right.span
    });
    for fact in &ordered_nodes {
        if !fact.ambient && fact.ty.has_hole() {
            return Err("typed preview node contains a concealed type hole".to_string());
        }
        bytes.extend_from_slice(&encode(&object([
            ("ambient", boolean(fact.ambient)),
            (
                "nodeKind",
                string(match fact.kind {
                    topaz_hir::TypedNodeKind::Expression => "expression",
                    topaz_hir::TypedNodeKind::Pattern => "pattern",
                    topaz_hir::TypedNodeKind::Binding => "binding",
                    topaz_hir::TypedNodeKind::Declaration => "declaration",
                    topaz_hir::TypedNodeKind::Type => "type",
                }),
            ),
            ("nodeId", node_id(fact.span)),
            ("rowKind", string("node")),
            ("schema", string(TYPED_SCHEMA)),
            ("sourceId", string(source(fact.span))),
            ("span", span(source(fact.span), fact.span)),
            ("type", semantic_type(&fact.ty)),
        ])));
    }
    let mut ordered_calls = calls.to_vec();
    ordered_calls.sort_by_key(|fact| {
        (
            fact.module.clone(),
            fact.span.file.0,
            fact.span.lo,
            fact.span.hi,
        )
    });
    for fact in &ordered_calls {
        if !fact.ambient && (fact.callee_type.has_hole() || fact.result_type.has_hole()) {
            return Err("typed preview call contains a concealed type hole".to_string());
        }
        bytes.extend_from_slice(&encode(&object([
            ("ambient", boolean(fact.ambient)),
            ("callNodeId", node_id(fact.span)),
            ("calleeNodeId", node_id(fact.callee_span)),
            (
                "calleeSpan",
                span(source(fact.callee_span), fact.callee_span),
            ),
            ("calleeType", semantic_type(&fact.callee_type)),
            ("plan", typed_call_plan(&fact.plan, &sources)),
            ("resultType", semantic_type(&fact.result_type)),
            ("rowKind", string("call")),
            ("schema", string(TYPED_SCHEMA)),
            ("sourceId", string(source(fact.span))),
            ("span", span(source(fact.span), fact.span)),
            (
                "targetIdentity",
                fact.target_identity
                    .as_ref()
                    .map_or(JsonValue::Null, string),
            ),
        ])));
    }
    let mut ordered_captures = captures.to_vec();
    ordered_captures.sort_by_key(|fact| {
        (
            fact.module.clone(),
            fact.closure_span.file.0,
            fact.closure_span.lo,
            fact.reference_span.lo,
            fact.name.clone(),
        )
    });
    ordered_captures.dedup_by(|left, right| {
        left.closure_span == right.closure_span
            && left.reference_span == right.reference_span
            && left.declaration_span == right.declaration_span
    });
    for fact in &ordered_captures {
        if !fact.ambient && fact.ty.has_hole() {
            return Err("typed preview capture contains a concealed type hole".to_string());
        }
        bytes.extend_from_slice(&encode(&object([
            ("ambient", boolean(fact.ambient)),
            ("closureNodeId", node_id(fact.closure_span)),
            ("declarationNodeId", node_id(fact.declaration_span)),
            ("name", string(&fact.name)),
            ("referenceNodeId", node_id(fact.reference_span)),
            ("rowKind", string("capture")),
            ("schema", string(TYPED_SCHEMA)),
            ("sourceId", string(source(fact.reference_span))),
            (
                "span",
                span(source(fact.reference_span), fact.reference_span),
            ),
            ("type", semantic_type(&fact.ty)),
        ])));
    }
    Ok(bytes)
}

/// Builds a typed-layer bundle from resolved rows and checked HIR observations.
pub fn build_typed_preview_observation(
    input: TypedPreviewObservationInput<'_>,
) -> Result<ObservationBundle, String> {
    let TypedPreviewObservationInput {
        resolved,
        nodes,
        calls,
        captures,
        diagnostics: check_diagnostics,
    } = input;
    let ResolvedPreviewObservationInput {
        request,
        modules,
        diagnostics,
        ..
    } = resolved;
    if request.terminal_phase() != crate::TerminalPhase::Typed {
        return Err("typed preview requires the typed terminal phase".to_string());
    }
    if !diagnostics.is_empty()
        && (!nodes.is_empty()
            || !calls.is_empty()
            || !captures.is_empty()
            || !check_diagnostics.is_empty())
    {
        return Err("resolved-rejected typed preview cannot carry checker output".to_string());
    }
    let observed_hir = (nodes.len() + calls.len() + captures.len()) as u64;
    if observed_hir > request.budgets().max_hir_nodes {
        return Err(format!(
            "hir-node resource limit: observed {observed_hir}, limit {}",
            request.budgets().max_hir_nodes
        ));
    }
    let ResolvedPreviewFiles {
        mut files,
        request_digest,
        node_ordinals,
    } = build_resolved_preview_files(resolved)?;
    let typed_bytes = if check_diagnostics.is_empty() {
        preview_typed_bytes(modules, &node_ordinals, nodes, calls, captures)?
    } else {
        Vec::new()
    };
    files.insert(
        "typed.jsonl".to_string(),
        (TYPED_SCHEMA.to_string(), typed_bytes),
    );
    let source_identity = |module_index: usize| -> Result<String, String> {
        let module = modules
            .get(module_index)
            .ok_or_else(|| format!("typed preview diagnostic references module {module_index}"))?;
        Ok(source_id(&module.identity, &module.path))
    };
    if diagnostics.is_empty() {
        let mut diagnostic_rows = Vec::new();
        for (ordinal, diagnostic) in check_diagnostics.iter().enumerate() {
            let source_id = source_identity(diagnostic.module_index)?;
            let secondary = diagnostic
                .secondary
                .iter()
                .map(|label| {
                    let label_source = source_identity(label.module_index)?;
                    Ok(object([
                        ("message", string(&label.message)),
                        (
                            "span",
                            object([
                                ("hi", unsigned(label.hi.into())),
                                ("lo", unsigned(label.lo.into())),
                                ("sourceId", string(label_source)),
                            ]),
                        ),
                    ]))
                })
                .collect::<Result<Vec<_>, String>>()?;
            diagnostic_rows.push(object([
                ("code", string(&diagnostic.code)),
                ("message", string(&diagnostic.message)),
                ("notes", array(diagnostic.notes.iter().map(string))),
                ("ordinal", unsigned(ordinal as u64)),
                (
                    "primary",
                    object([
                        ("message", string(&diagnostic.primary_message)),
                        (
                            "span",
                            object([
                                ("hi", unsigned(diagnostic.hi.into())),
                                ("lo", unsigned(diagnostic.lo.into())),
                                ("sourceId", string(&source_id)),
                            ]),
                        ),
                    ]),
                ),
                ("producerPhase", string("front-end")),
                (
                    "profileRule",
                    diagnostic
                        .profile_rule
                        .as_ref()
                        .map_or(JsonValue::Null, string),
                ),
                ("schema", string(DIAGNOSTICS_SCHEMA)),
                ("secondary", array(secondary)),
                ("severity", string("error")),
            ]));
        }
        files.insert(
            "diagnostics.jsonl".to_string(),
            (
                DIAGNOSTICS_SCHEMA.to_string(),
                if diagnostic_rows.is_empty() {
                    Vec::new()
                } else {
                    encode_jsonl(&diagnostic_rows)
                },
            ),
        );
    }
    let resolved_rejected = !diagnostics.is_empty();
    let rejected = resolved_rejected || !check_diagnostics.is_empty();
    if rejected {
        files.insert(
            "lowered.jsonl".to_string(),
            (LOWERED_SCHEMA.to_string(), Vec::new()),
        );
        files.insert(
            "rust-source.jsonl".to_string(),
            (RUST_SOURCE_SCHEMA.to_string(), Vec::new()),
        );
    }

    let response = encode(&object([
        (
            "highestCompletedPhase",
            string(if resolved_rejected {
                "resolved"
            } else {
                "typed"
            }),
        ),
        (
            "phases",
            object([
                ("ast", string("produced")),
                (
                    "lowered",
                    string(if rejected { "blocked" } else { "not-requested" }),
                ),
                ("resolved", string("produced")),
                (
                    "rustSource",
                    string(if rejected { "blocked" } else { "not-requested" }),
                ),
                ("tokens", string("produced")),
                (
                    "typed",
                    string(if resolved_rejected {
                        "blocked"
                    } else {
                        "produced"
                    }),
                ),
            ]),
        ),
        (
            "projectionDigests",
            object([
                ("ast", string(sha256(&files["ast.jsonl"].1))),
                ("diagnostics", string(sha256(&files["diagnostics.jsonl"].1))),
                (
                    "lowered",
                    if rejected {
                        string(sha256(&files["lowered.jsonl"].1))
                    } else {
                        JsonValue::Null
                    },
                ),
                ("resolved", string(sha256(&files["resolved.jsonl"].1))),
                (
                    "rustSource",
                    if rejected {
                        string(sha256(&files["rust-source.jsonl"].1))
                    } else {
                        JsonValue::Null
                    },
                ),
                ("sourceSet", string(sha256(&files["source-set.jsonl"].1))),
                ("tokens", string(sha256(&files["tokens.jsonl"].1))),
                ("typed", string(sha256(&files["typed.jsonl"].1))),
            ]),
        ),
        ("requestDigest", string(request_digest)),
        ("schema", string(crate::RESPONSE_SCHEMA)),
        (
            "status",
            string(if rejected { "rejected" } else { "completed" }),
        ),
    ]));
    files.insert(
        "response.json".to_string(),
        (crate::RESPONSE_SCHEMA.to_string(), response),
    );
    // Every row above is constructed through the canonical schema writers.
    // Avoid reparsing the complete typed bundle twice in the producing
    // process; `compiler validate` remains the independent consumer gate.
    finish_observation(files, request.budgets().max_projection_bytes, false)
}
