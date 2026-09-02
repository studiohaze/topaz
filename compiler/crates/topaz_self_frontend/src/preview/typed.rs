use crate::*;

/// Keeps checker rows aligned with the resolved graph from which they were derived.
pub struct TypedPreviewResult {
    pub resolved: ResolvedPreviewResult,
    pub nodes: Vec<topaz_hir::TypedNode>,
    pub calls: Vec<topaz_hir::TypedCall>,
    pub captures: Vec<topaz_hir::TypedCapture>,
    pub diagnostics: Vec<topaz_kernel::CanonicalPreviewCheckDiagnostic>,
}

impl TypedPreviewResult {
    /// Borrows the resolved and typed rows consumed by kernel observations.
    pub fn observation_input(&self) -> topaz_kernel::TypedPreviewObservationInput<'_> {
        topaz_kernel::TypedPreviewObservationInput {
            resolved: self.resolved.observation_input(),
            nodes: &self.nodes,
            calls: &self.calls,
            captures: &self.captures,
            diagnostics: &self.diagnostics,
        }
    }

    /// Return the exact checked type fact for one resolved value export.
    ///
    /// Functions own declaration facts, while `let` and `const` names own
    /// binding facts. Both use the resolver's declaration span as their
    /// module-stable identity.
    pub fn exported_value_node(
        &self,
        export: &topaz_kernel::CanonicalPreviewResolvedExport,
    ) -> Option<&topaz_hir::TypedNode> {
        if export.namespace != "value" {
            return None;
        }
        let module = self.resolved.modules.get(export.module_index)?;
        self.nodes.iter().find(|node| {
            node.module == module.identity
                && matches!(
                    node.kind,
                    topaz_hir::TypedNodeKind::Binding | topaz_hir::TypedNodeKind::Declaration
                )
                && node.span.lo == export.declaration_lo
                && node.span.hi == export.declaration_hi
        })
    }
}

#[derive(Debug)]
pub(crate) struct FlatTypeAtom {
    parent: i64,
    field: String,
    index: usize,
    edge_name: String,
    kind: String,
    name: String,
    value: String,
    identity: String,
    origin: String,
}

pub(crate) fn parse_flat_type_atoms(
    value: &JsonValue,
    label: &str,
) -> Result<Vec<FlatTypeAtom>, String> {
    let JsonValue::Array(values) = value else {
        return Err(format!("front-end checker {label} type is not an array"));
    };
    let atoms = values
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = exact_object(
                value,
                &format!("{label} type atom {ordinal}"),
                &[
                    "parent", "field", "index", "edgeName", "kind", "name", "value", "identity",
                    "origin",
                ],
            )?;
            Ok(FlatTypeAtom {
                parent: json_i64(object, "parent")?,
                field: json_string_field(object, "field")?.to_string(),
                index: usize::try_from(json_i64(object, "index")?)
                    .map_err(|_| format!("front-end checker {label} type index is negative"))?,
                edge_name: json_string_field(object, "edgeName")?.to_string(),
                kind: json_string_field(object, "kind")?.to_string(),
                name: json_string_field(object, "name")?.to_string(),
                value: json_string_field(object, "value")?.to_string(),
                identity: json_string_field(object, "identity")?.to_string(),
                origin: json_string_field(object, "origin")?.to_string(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if atoms.len() != 1 && atoms.iter().filter(|atom| atom.parent == -1).count() != 1 {
        return Err(format!(
            "front-end checker {label} type does not have one root"
        ));
    }
    Ok(atoms)
}

pub(crate) fn type_children<'a>(
    atoms: &'a [FlatTypeAtom],
    parent: usize,
    field: &str,
) -> Result<Vec<(usize, &'a FlatTypeAtom)>, String> {
    let mut values = atoms
        .iter()
        .enumerate()
        .filter(|(_, atom)| atom.parent == parent as i64 && atom.field == field)
        .collect::<Vec<_>>();
    values.sort_by_key(|(_, atom)| atom.index);
    if values
        .iter()
        .enumerate()
        .any(|(index, (_, atom))| atom.index != index)
    {
        return Err(format!(
            "front-end checker type `{field}` child indices are not contiguous"
        ));
    }
    Ok(values)
}

pub(crate) fn semantic_type_at(
    atoms: &[FlatTypeAtom],
    ordinal: usize,
    observing_module: &str,
    canonical_nominals: bool,
) -> Result<topaz_hir::SemanticType, String> {
    use topaz_hir::{
        SemanticConstructor as C, SemanticField, SemanticLiteral as L, SemanticPrimitive as P,
        SemanticType as T,
    };
    let atom = atoms
        .get(ordinal)
        .ok_or_else(|| "front-end checker type ordinal is outside the tree".to_string())?;
    let nested = |field: &str| -> Result<Vec<T>, String> {
        type_children(atoms, ordinal, field)?
            .into_iter()
            .map(|(index, _)| semantic_type_at(atoms, index, observing_module, canonical_nominals))
            .collect()
    };
    let nominal_identity = || {
        if canonical_nominals
            && !atom.origin.is_empty()
            && atom.origin != observing_module
            && !atom.identity.contains("::")
        {
            format!("{}::{}", atom.origin, atom.identity)
        } else {
            atom.identity.clone()
        }
    };
    let result = match atom.kind.as_str() {
        "primitive" => T::Primitive(match atom.name.as_str() {
            "int" => P::Int,
            "float" => P::Float,
            "string" => P::String,
            "bool" => P::Bool,
            "unit" => P::Unit,
            name => return Err(format!("front-end checker unknown primitive `{name}`")),
        }),
        "literal" => T::Literal(match atom.name.as_str() {
            "string" => L::String(atom.value.clone()),
            "int" => L::Int(
                atom.value
                    .parse()
                    .map_err(|_| "front-end checker integer literal is invalid".to_string())?,
            ),
            "float" => L::Float(atom.value.clone()),
            "bool" if atom.value == "true" => L::Bool(true),
            "bool" if atom.value == "false" => L::Bool(false),
            "null" => L::Null,
            name => return Err(format!("front-end checker unknown literal `{name}`")),
        }),
        "union" => T::Union(nested("members")?),
        "record" => {
            let fields = type_children(atoms, ordinal, "fields")?
                .into_iter()
                .map(|(index, atom)| {
                    Ok(SemanticField {
                        name: atom.edge_name.clone(),
                        ty: semantic_type_at(atoms, index, observing_module, canonical_nominals)?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            T::Record(fields)
        }
        "constructor" => T::Constructor {
            constructor: match atom.name.as_str() {
                "Array" => C::Array,
                "Map" => C::Map,
                "Set" => C::Set,
                "Option" => C::Option,
                "Result" => C::Result,
                "Range" => C::Range,
                name => {
                    return Err(format!(
                        "front-end checker unknown semantic constructor `{name}`"
                    ));
                }
            },
            arguments: nested("arguments")?,
        },
        "function" => {
            let result = nested("result")?;
            if result.len() != 1 {
                return Err("front-end checker function type needs one result".to_string());
            }
            let variadic = nested("variadic")?;
            if variadic.len() > 1 {
                return Err(
                    "front-end checker function type has multiple variadic types".to_string(),
                );
            }
            T::Function {
                parameters: nested("parameters")?,
                variadic: variadic.into_iter().next().map(Box::new),
                result: Box::new(result.into_iter().next().expect("one result")),
            }
        }
        "foreign" => T::Foreign {
            identity: atom.identity.clone(),
            arguments: nested("arguments")?,
        },
        "rigid" => T::Rigid {
            name: atom.name.clone(),
            origin: atom.origin.clone(),
        },
        "enum" => T::Enum {
            identity: nominal_identity(),
            arguments: nested("arguments")?,
        },
        "nominal-record" => T::NominalRecord {
            identity: nominal_identity(),
            arguments: nested("arguments")?,
        },
        "newtype" => T::Newtype {
            identity: nominal_identity(),
            arguments: nested("arguments")?,
        },
        "template" => T::Template,
        "file" => T::File,
        "json-value" => T::JsonValue,
        "bytes" => T::Bytes,
        "byte-buffer" => T::ByteBuffer,
        "path" => T::Path,
        "regex" => T::Regex,
        "match" => T::Match,
        "toml-value" => T::TomlValue,
        "url" => T::Url,
        "date" => T::Date,
        "big-int" => T::BigInt,
        "decimal" => T::Decimal,
        "rounding-mode" => T::RoundingMode,
        "inference-variable" => T::InferenceVariable,
        "unknown" => T::Unknown,
        kind => return Err(format!("front-end checker unknown type kind `{kind}`")),
    };
    Ok(result)
}

pub(crate) fn parse_semantic_type(
    value: &JsonValue,
    label: &str,
    observing_module: &str,
    canonical_nominals: bool,
) -> Result<topaz_hir::SemanticType, String> {
    let atoms = parse_flat_type_atoms(value, label)?;
    let root = atoms
        .iter()
        .position(|atom| atom.parent == -1)
        .ok_or_else(|| format!("front-end checker {label} type root is missing"))?;
    let reachable = semantic_type_at(&atoms, root, observing_module, canonical_nominals)?;
    Ok(reachable)
}

pub(crate) fn checker_span(
    module_index: usize,
    object: &JsonObject,
) -> Result<topaz_diag::Span, String> {
    Ok(topaz_diag::Span::new(
        topaz_diag::FileId(
            u32::try_from(module_index)
                .map_err(|_| "front-end checker module index exceeds u32".to_string())?,
        ),
        json_u32(object, "lo")?,
        json_u32(object, "hi")?,
    ))
}

pub(crate) fn checker_module(
    modules: &[topaz_kernel::CanonicalPreviewModule],
    module_index: usize,
) -> Result<&str, String> {
    modules
        .get(module_index)
        .map(|module| module.identity.as_str())
        .ok_or_else(|| format!("front-end checker module index {module_index} is invalid"))
}

pub(crate) fn parse_typed_nodes(
    root: &JsonObject,
    modules: &[topaz_kernel::CanonicalPreviewModule],
    version: topaz_syntax::LangVersion,
) -> Result<Vec<topaz_hir::TypedNode>, String> {
    json_array_field(root, "typedNodes")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = exact_object(
                value,
                &format!("typed node {ordinal}"),
                &[
                    "moduleIndex",
                    "nodeKind",
                    "lo",
                    "hi",
                    "typeValue",
                    "ambient",
                ],
            )?;
            let module_index = usize::try_from(json_i64(object, "moduleIndex")?)
                .map_err(|_| format!("front-end checker node {ordinal} module is negative"))?;
            let module = checker_module(modules, module_index)?;
            let ty = parse_semantic_type(
                object
                    .get("typeValue")
                    .ok_or_else(|| "front-end checker node type is missing".to_string())?,
                &format!("node {ordinal}"),
                module,
                version >= topaz_syntax::LangVersion::V5_20,
            )?;
            let ambient = json_bool_field(object, "ambient")?;
            if !ambient && ty.has_hole() {
                return Err(format!(
                    "front-end checker node {ordinal} hides a type hole"
                ));
            }
            Ok(topaz_hir::TypedNode {
                module: module.to_string(),
                kind: match json_string_field(object, "nodeKind")? {
                    "expression" => topaz_hir::TypedNodeKind::Expression,
                    "pattern" => topaz_hir::TypedNodeKind::Pattern,
                    "binding" => topaz_hir::TypedNodeKind::Binding,
                    "declaration" => topaz_hir::TypedNodeKind::Declaration,
                    "type" => topaz_hir::TypedNodeKind::Type,
                    kind => {
                        return Err(format!(
                            "front-end checker node {ordinal} has unknown kind `{kind}`"
                        ));
                    }
                },
                span: checker_span(module_index, object)?,
                ty,
                ambient,
            })
        })
        .collect()
}

pub(crate) fn parse_call_arguments(
    object: &JsonObject,
    module_index: usize,
    ordinal: usize,
) -> Result<Vec<topaz_hir::ArgPlan>, String> {
    json_array_field(object, "arguments")?
        .iter()
        .enumerate()
        .map(|(argument_index, value)| {
            let argument = exact_object(
                value,
                &format!("typed call {ordinal} argument {argument_index}"),
                &["bindingKind", "bindingName", "sourceIndex", "lo", "hi"],
            )?;
            Ok(topaz_hir::ArgPlan {
                source_index: match argument.get("sourceIndex") {
                    Some(JsonValue::Null) => None,
                    Some(_) => Some(usize::try_from(json_i64(argument, "sourceIndex")?).map_err(
                        |_| {
                            format!(
                                "front-end checker call {ordinal} argument source index is negative"
                            )
                        },
                    )?),
                    None => {
                        return Err(format!(
                            "front-end checker call {ordinal} argument source index is missing"
                        ));
                    }
                },
                binding: match json_string_field(argument, "bindingKind")? {
                    "positional" => topaz_hir::ArgBinding::Positional,
                    "named" => topaz_hir::ArgBinding::Named(
                        json_string_field(argument, "bindingName")?.to_string(),
                    ),
                    "spread" => topaz_hir::ArgBinding::Spread,
                    "inserted-lead" => topaz_hir::ArgBinding::InsertedLead,
                    kind => {
                        return Err(format!(
                            "front-end checker call {ordinal} has unknown argument binding `{kind}`"
                        ));
                    }
                },
                span: checker_span(module_index, argument)?,
            })
        })
        .collect()
}

pub(crate) fn parse_call_evaluation(
    object: &JsonObject,
    ordinal: usize,
) -> Result<Vec<topaz_hir::EvalStep>, String> {
    json_array_field(object, "evaluations")?
        .iter()
        .enumerate()
        .map(|(step_index, value)| {
            let step = exact_object(
                value,
                &format!("typed call {ordinal} evaluation {step_index}"),
                &["kind", "argumentIndex"],
            )?;
            match json_string_field(step, "kind")? {
                "callee" => Ok(topaz_hir::EvalStep::Callee),
                "receiver" => Ok(topaz_hir::EvalStep::Receiver),
                "optional-guard" => Ok(topaz_hir::EvalStep::OptionalGuard),
                "pipe-lead" => Ok(topaz_hir::EvalStep::PipeLead),
                "argument" => Ok(topaz_hir::EvalStep::Arg(
                    usize::try_from(json_i64(step, "argumentIndex")?).map_err(|_| {
                        format!("front-end checker call {ordinal} evaluation argument is negative")
                    })?,
                )),
                kind => Err(format!(
                    "front-end checker call {ordinal} has unknown evaluation `{kind}`"
                )),
            }
        })
        .collect()
}

pub(crate) fn parse_typed_calls(
    root: &JsonObject,
    modules: &[topaz_kernel::CanonicalPreviewModule],
    version: topaz_syntax::LangVersion,
) -> Result<Vec<topaz_hir::TypedCall>, String> {
    json_array_field(root, "typedCalls")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = typed_call_object(value, ordinal)?;
            let module_index = usize::try_from(json_i64(object, "moduleIndex")?)
                .map_err(|_| format!("front-end checker call {ordinal} module is negative"))?;
            let module = checker_module(modules, module_index)?;
            let callee_type = parse_semantic_type(
                object
                    .get("calleeType")
                    .ok_or_else(|| "front-end checker callee type is missing".to_string())?,
                &format!("call {ordinal} callee"),
                module,
                version >= topaz_syntax::LangVersion::V5_20,
            )?;
            let result_type = parse_semantic_type(
                object
                    .get("resultType")
                    .ok_or_else(|| "front-end checker result type is missing".to_string())?,
                &format!("call {ordinal} result"),
                module,
                version >= topaz_syntax::LangVersion::V5_20,
            )?;
            let ambient = json_bool_field(object, "ambient")?;
            if !ambient && (callee_type.has_hole() || result_type.has_hole()) {
                return Err(format!(
                    "front-end checker call {ordinal} at module {module_index}:{}-{} hides a type hole",
                    json_u32(object, "lo")?,
                    json_u32(object, "hi")?,
                ));
            }
            let plan_span = checker_span(module_index, object)?;
            let callee_span = topaz_diag::Span::new(
                topaz_diag::FileId(u32::try_from(module_index).map_err(|_| {
                    "front-end checker call module index exceeds u32".to_string()
                })?),
                json_u32(object, "calleeLo")?,
                json_u32(object, "calleeHi")?,
            );
            let callee = match json_string_field(object, "calleeKind")? {
                "value" => topaz_hir::CalleePlan::Value,
                "member" => topaz_hir::CalleePlan::Member {
                    method: json_string_field(object, "method")?.to_string(),
                    class: match json_string_field(object, "methodClass")? {
                        "higher-order" => topaz_hir::MethodClass::Hof,
                        "lazy-callback" => topaz_hir::MethodClass::LazyCallback,
                        "mutator" => topaz_hir::MethodClass::Mutator,
                        "resource" => topaz_hir::MethodClass::Resource,
                        "other" => topaz_hir::MethodClass::Other,
                        class => {
                            return Err(format!(
                                "front-end checker call {ordinal} has unknown method class `{class}`"
                            ));
                        }
                    },
                    optional: json_bool_field(object, "optional")?,
                    shadow_first: json_bool_field(object, "shadowFirst")?,
                },
                "pipe" => topaz_hir::CalleePlan::Pipe {
                    stage_method: optional_string(object, "stageMethod")?,
                },
                kind => {
                    return Err(format!(
                        "front-end checker call {ordinal} has unknown callee kind `{kind}`"
                    ));
                }
            };
            Ok(topaz_hir::TypedCall {
                module: module.to_string(),
                span: plan_span,
                callee_span,
                callee_type,
                result_type,
                target_identity: optional_string(object, "targetIdentity")?,
                ambient,
                plan: topaz_hir::CallPlan {
                    span: plan_span,
                    callee_span,
                    callee,
                    eval: parse_call_evaluation(object, ordinal)?,
                    args: parse_call_arguments(object, module_index, ordinal)?,
                },
            })
        })
        .collect()
}

pub(crate) const TYPED_CALL_FIELDS: &[&str] = &[
    "moduleIndex",
    "lo",
    "hi",
    "calleeLo",
    "calleeHi",
    "calleeType",
    "resultType",
    "targetIdentity",
    "ambient",
    "calleeKind",
    "method",
    "methodClass",
    "optional",
    "shadowFirst",
    "stageMethod",
    "arguments",
    "evaluations",
];

pub(crate) fn typed_call_object(value: &JsonValue, ordinal: usize) -> Result<&JsonObject, String> {
    let JsonValue::Object(object) = value else {
        return Err(format!(
            "front-end resolver typed call {ordinal} is not an object"
        ));
    };
    let has_current_fields = TYPED_CALL_FIELDS
        .iter()
        .all(|field| object.contains_key(*field));
    if object.len() == TYPED_CALL_FIELDS.len() && has_current_fields {
        return Ok(object);
    }
    if object.len() == TYPED_CALL_FIELDS.len() + 2
        && has_current_fields
        && object.contains_key("bindingKind")
        && object.contains_key("unsupportedReason")
    {
        if json_string_field(object, "bindingKind")? != "runtime"
            || !json_string_field(object, "unsupportedReason")?.is_empty()
        {
            return Err(format!(
                "front-end checker call {ordinal} uses an unsupported binding mode"
            ));
        }
        return Ok(object);
    }
    Err(format!(
        "front-end resolver typed call {ordinal} fields drifted: expected current fields {TYPED_CALL_FIELDS:?} or the sealed-image runtime binding suffix, found {:?}",
        object.keys().collect::<Vec<_>>()
    ))
}

pub(crate) fn parse_typed_captures(
    root: &JsonObject,
    modules: &[topaz_kernel::CanonicalPreviewModule],
    version: topaz_syntax::LangVersion,
) -> Result<Vec<topaz_hir::TypedCapture>, String> {
    json_array_field(root, "typedCaptures")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = exact_object(
                value,
                &format!("typed capture {ordinal}"),
                &[
                    "moduleIndex",
                    "closureLo",
                    "closureHi",
                    "referenceLo",
                    "referenceHi",
                    "declarationLo",
                    "declarationHi",
                    "name",
                    "typeValue",
                    "ambient",
                ],
            )?;
            let module_index = usize::try_from(json_i64(object, "moduleIndex")?)
                .map_err(|_| format!("front-end checker capture {ordinal} module is negative"))?;
            let file = topaz_diag::FileId(
                u32::try_from(module_index)
                    .map_err(|_| "front-end checker capture module exceeds u32".to_string())?,
            );
            let module = checker_module(modules, module_index)?;
            let ty = parse_semantic_type(
                object
                    .get("typeValue")
                    .ok_or_else(|| "front-end checker capture type is missing".to_string())?,
                &format!("capture {ordinal}"),
                module,
                version >= topaz_syntax::LangVersion::V5_20,
            )?;
            let ambient = json_bool_field(object, "ambient")?;
            if !ambient && ty.has_hole() {
                return Err(format!(
                    "front-end checker capture {ordinal} hides a type hole"
                ));
            }
            Ok(topaz_hir::TypedCapture {
                module: module.to_string(),
                closure_span: topaz_diag::Span::new(
                    file,
                    json_u32(object, "closureLo")?,
                    json_u32(object, "closureHi")?,
                ),
                reference_span: topaz_diag::Span::new(
                    file,
                    json_u32(object, "referenceLo")?,
                    json_u32(object, "referenceHi")?,
                ),
                declaration_span: topaz_diag::Span::new(
                    file,
                    json_u32(object, "declarationLo")?,
                    json_u32(object, "declarationHi")?,
                ),
                name: json_string_field(object, "name")?.to_string(),
                ty,
                ambient,
            })
        })
        .collect()
}

pub(crate) fn parse_checker_diagnostics(
    root: &JsonObject,
) -> Result<Vec<topaz_kernel::CanonicalPreviewCheckDiagnostic>, String> {
    json_array_field(root, "checkerDiagnostics")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = exact_object(
                value,
                &format!("checker diagnostic {ordinal}"),
                &[
                    "moduleIndex",
                    "code",
                    "message",
                    "primaryMessage",
                    "lo",
                    "hi",
                    "secondary",
                    "notes",
                    "profileRule",
                ],
            )?;
            let secondary = json_array_field(object, "secondary")?
                .iter()
                .enumerate()
                .map(|(label_ordinal, value)| {
                    let label = exact_object(
                        value,
                        &format!("checker diagnostic {ordinal} label {label_ordinal}"),
                        &["moduleIndex", "lo", "hi", "message"],
                    )?;
                    Ok(topaz_kernel::CanonicalPreviewCheckLabel {
                        module_index: usize::try_from(json_i64(label, "moduleIndex")?).map_err(
                            |_| {
                                format!(
                                    "front-end checker diagnostic {ordinal} label module is negative"
                                )
                            },
                        )?,
                        lo: json_u32(label, "lo")?,
                        hi: json_u32(label, "hi")?,
                        message: json_string_field(label, "message")?.to_string(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let notes = json_string_array_field(
                object,
                "notes",
                &format!("checker diagnostic {ordinal}"),
            )?;
            Ok(topaz_kernel::CanonicalPreviewCheckDiagnostic {
                module_index: usize::try_from(json_i64(object, "moduleIndex")?).map_err(|_| {
                    format!("front-end checker diagnostic {ordinal} module is negative")
                })?,
                code: json_string_field(object, "code")?.to_string(),
                message: json_string_field(object, "message")?.to_string(),
                primary_message: json_string_field(object, "primaryMessage")?.to_string(),
                lo: json_u32(object, "lo")?,
                hi: json_u32(object, "hi")?,
                secondary,
                notes,
                profile_rule: optional_string(object, "profileRule")?,
            })
        })
        .collect()
}

/// Checks a fact-backed package through a reusable embedded front-end session.
pub fn preview_typed_with(
    session: &FrontEndSession,
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<TypedPreviewResult, String> {
    if request.terminal_phase() != topaz_kernel::TerminalPhase::Typed {
        return Err("front-end checker preview requires the typed terminal phase".to_string());
    }
    let mut resolved = preview_resolved_or_typed_with(session, source, request)?;
    let checker = resolved
        .checker
        .take()
        .ok_or_else(|| "front-end typed preview omitted checker projection".to_string())?;
    Ok(TypedPreviewResult {
        resolved,
        nodes: checker.nodes,
        calls: checker.calls,
        captures: checker.captures,
        diagnostics: checker.diagnostics,
    })
}

/// Checks a fact-backed package with a fresh embedded compiler session.
pub fn preview_typed(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<TypedPreviewResult, String> {
    preview_typed_with(&FrontEndSession::new()?, source, request)
}

pub(crate) fn preview_compiler_typed_by(
    invoke: impl Fn(&[u8]) -> Result<Vec<u8>, String>,
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
    producer: CompilerProducer,
    selected_profile: Option<CompilationProfile>,
) -> Result<TypedPreviewResult, String> {
    if request.terminal_phase() != topaz_kernel::TerminalPhase::Typed {
        return Err("compiler typed product requires the typed terminal phase".to_string());
    }
    let profile = match selected_profile {
        Some(CompilationProfile::None) => {
            return Err("profiled self compilation requires a named profile".to_string());
        }
        Some(profile) => profile,
        None => CompilationProfile::None,
    };
    // The fixed Stage 2 producer accepts lowered or generated-source requests.
    // Ask it for the lowered envelope, validate that envelope and mechanically
    // extract its typed front-end member; no Rust target phase participates.
    let mut request = request.with_terminal_phase(topaz_kernel::TerminalPhase::Lowered);
    let max_rounds = request
        .budgets()
        .max_source_facts
        .saturating_mul(3)
        .saturating_add(4);
    let mut rounds = 0u64;
    loop {
        if rounds >= max_rounds {
            return Err(format!(
                "compiler typed-product fact rounds exceed {max_rounds}"
            ));
        }
        rounds += 1;
        let encoded = encode_compiler_request_with_profile(&request, producer, profile)?;
        let response = invoke(&encoded)?;
        let root = decode_stage1_response_root(&response, "compiler typed-product response")?;
        let status = json_string_field(&root, "status")?;
        let front_end_text = json_string_field(&root, "frontEnd")?;
        let front_end = decode_front_end_response_text(
            front_end_text,
            "compiler typed-product front-end member",
        )?;
        if json_string_field(&front_end, "status")? != status {
            return Err(
                "compiler typed-product status contradicts its front-end member".to_string(),
            );
        }
        let _lowered_modules = json_array_field(&root, "loweredModules")?;
        let _lowered_operations = json_array_field(&root, "loweredOperations")?;
        if !json_string_field(&root, "generatedRust")?.is_empty()
            || !json_array_field(&root, "unsupported")?.is_empty()
        {
            return Err(
                "compiler typed-product envelope crossed the selected check boundary".to_string(),
            );
        }
        let provenance = exact_object(
            root.get("provenance")
                .ok_or_else(|| "compiler typed-product omitted provenance".to_string())?,
            "compiler typed-product provenance",
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
            return Err("compiler typed-product carries the wrong stage identity".to_string());
        }
        if json_string_field(provenance, "sourceSetId")? != source_set_id() {
            return Err("compiler typed-product source-set identity drifted".to_string());
        }

        let queries = parse_queries(&front_end)?;
        if advance_compiler_fact_round(
            source,
            &mut request,
            status,
            queries,
            "compiler typed-product",
        )? {
            continue;
        }
        if !matches!(status, "completed" | "rejected") {
            return Err(format!(
                "compiler typed-product returned invalid status `{status}`"
            ));
        }
        return decode_stage1_typed_preview(&request, front_end_text, rounds);
    }
}

/// Execute the exact embedded C2 program image to obtain a current-mode typed
/// product from its lowered envelope. This ordinary self-compiler bridge does
/// not construct a Rust resolver/checker result and does not retry Stage 0.
pub fn preview_linked_stage2_typed(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<TypedPreviewResult, String> {
    preview_compiler_typed_by(
        topaz_stage1_runtime::execute_embedded_stage2_compiler,
        source,
        request,
        CompilerProducer::Stage2,
        None,
    )
}

/// Execute the exact embedded C2 program image with a named compilation
/// profile while retaining the typed product used by language-server features.
pub fn preview_linked_stage2_profiled_typed(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
    profile: CompilationProfile,
) -> Result<TypedPreviewResult, String> {
    preview_compiler_typed_by(
        topaz_stage1_runtime::execute_embedded_stage2_compiler,
        source,
        request,
        CompilerProducer::Stage2,
        Some(profile),
    )
}

/// Decodes the typed projection from a sealed Stage 1 front-end response.
pub fn decode_stage1_typed_preview(
    request: &topaz_kernel::KernelRequest,
    front_end: &str,
    rounds: u64,
) -> Result<TypedPreviewResult, String> {
    let root = decode_front_end_response_text(front_end, "Stage 1 front-end response")?;
    decode_stage1_typed_root(request, &root, rounds, ResolvedDiagnosticShape::SealedImage)
}

/// Reuses a generated result's retained response root for typed projection.
pub fn decode_stage1_typed_from_generated(
    result: &Stage1GeneratedPreviewResult,
) -> Result<TypedPreviewResult, String> {
    decode_stage1_typed_root(
        &result.request,
        &result.front_end_root,
        result.rounds,
        result.resolved_diagnostic_shape,
    )
}

pub(crate) fn decode_stage1_typed_root(
    request: &topaz_kernel::KernelRequest,
    root: &JsonObject,
    rounds: u64,
    diagnostic_shape: ResolvedDiagnosticShape,
) -> Result<TypedPreviewResult, String> {
    let status = json_string_field(root, "status")?;
    if !matches!(status, "completed" | "rejected") || !parse_queries(root)?.is_empty() {
        return Err(
            "Stage 1 final front end has an incomplete status or pending query".to_string(),
        );
    }
    let modules = parse_modules(root)?;
    let edges = parse_edges(root)?;
    let scopes = parse_scopes(root)?;
    let declarations = parse_declarations(root)?;
    let references = parse_references(root)?;
    let exports = parse_exports(root)?;
    let diagnostics = parse_resolved_diagnostics_with(root, diagnostic_shape)?;
    let checker_diagnostics = parse_checker_diagnostics(root)?;
    let checker_has_only_profile_denials = checker_diagnostics
        .iter()
        .all(|diagnostic| diagnostic.profile_rule.is_some());
    let (nodes, calls, captures) = if diagnostics.is_empty() && checker_has_only_profile_denials {
        (
            parse_typed_nodes(root, &modules, request.language_version())?,
            parse_typed_calls(root, &modules, request.language_version())?,
            parse_typed_captures(root, &modules, request.language_version())?,
        )
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };
    let typed_request = request
        .clone()
        .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    enforce_resolver_budgets(
        &typed_request,
        &modules,
        &scopes,
        &declarations,
        &references,
        &exports,
        &diagnostics,
    )?;
    let rejected = !diagnostics.is_empty() || !checker_diagnostics.is_empty();
    if (status == "completed" && rejected) || (status == "rejected" && !rejected) {
        return Err("Stage 1 front-end status contradicts diagnostics".to_string());
    }
    Ok(TypedPreviewResult {
        resolved: ResolvedPreviewResult {
            request: typed_request,
            modules,
            edges,
            scopes,
            declarations,
            references,
            exports,
            diagnostics,
            rounds,
            checker: None,
        },
        nodes,
        calls,
        captures,
        diagnostics: checker_diagnostics,
    })
}
