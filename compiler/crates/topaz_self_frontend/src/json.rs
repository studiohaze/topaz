use crate::*;

pub(crate) type JsonObject = std::collections::BTreeMap<std::rc::Rc<str>, JsonValue>;

pub(crate) const FRONT_END_RESPONSE_FIELDS: [&str; 19] = [
    "schema",
    "status",
    "sourceId",
    "raw",
    "layout",
    "ast",
    "diagnostics",
    "queries",
    "modules",
    "edges",
    "scopes",
    "declarations",
    "references",
    "exports",
    "resolverDiagnostics",
    "typedNodes",
    "typedCalls",
    "typedCaptures",
    "checkerDiagnostics",
];

pub(crate) fn decode_front_end_response_text(
    text: &str,
    context: &str,
) -> Result<Rc<JsonObject>, String> {
    let parsed = json_parse(text).map_err(|error| format!("{context} is not JSON: {error:?}"))?;
    let root = exact_object(&parsed, context, &FRONT_END_RESPONSE_FIELDS)?;
    expect_json_string(root, "schema", EXCHANGE_SCHEMA)?;
    let JsonValue::Object(root) = parsed else {
        unreachable!("exact front-end response object was validated above")
    };
    Ok(root)
}

pub(crate) fn decode_front_end_response_bytes(
    response: &[u8],
    context: &str,
) -> Result<Rc<JsonObject>, String> {
    let text = std::str::from_utf8(response)
        .map_err(|error| format!("{context} is not UTF-8: {error}"))?;
    decode_front_end_response_text(text, context)
}

pub(crate) fn json_array_field<'a>(
    object: &'a JsonObject,
    field: &str,
) -> Result<&'a [JsonValue], String> {
    match object.get(field) {
        Some(JsonValue::Array(values)) => Ok(values),
        _ => Err(format!("front-end resolver `{field}` is not an array")),
    }
}

pub(crate) fn json_bool_field(object: &JsonObject, field: &str) -> Result<bool, String> {
    match object.get(field) {
        Some(JsonValue::Bool(value)) => Ok(*value),
        _ => Err(format!("front-end resolver `{field}` is not boolean")),
    }
}

pub(crate) fn exact_object<'a>(
    value: &'a JsonValue,
    label: &str,
    fields: &[&str],
) -> Result<&'a JsonObject, String> {
    let JsonValue::Object(object) = value else {
        return Err(format!("front-end resolver {label} is not an object"));
    };
    if object.len() != fields.len() || !fields.iter().all(|field| object.contains_key(*field)) {
        return Err(format!(
            "front-end resolver {label} fields drifted: expected {fields:?}, found {:?}",
            object.keys().collect::<Vec<_>>()
        ));
    }
    Ok(object)
}

pub(crate) fn parse_queries(root: &JsonObject) -> Result<Vec<topaz_kernel::HostQuery>, String> {
    json_array_field(root, "queries")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = exact_object(
                value,
                &format!("query {ordinal}"),
                &["kind", "mountId", "logicalPath"],
            )?;
            let mount_id = json_string_field(object, "mountId")?.to_string();
            let logical_path =
                topaz_resolve::normalize_path(json_string_field(object, "logicalPath")?);
            match json_string_field(object, "kind")? {
                "read-source" => Ok(topaz_kernel::HostQuery::ReadSource {
                    mount_id,
                    logical_path,
                }),
                "list-directory" => Ok(topaz_kernel::HostQuery::ListDirectory {
                    mount_id,
                    logical_path,
                }),
                "physical-containment" => Ok(topaz_kernel::HostQuery::PhysicalContainment {
                    mount_id,
                    logical_path,
                }),
                kind => Err(format!(
                    "front-end resolver query {ordinal} has unknown kind `{kind}`"
                )),
            }
        })
        .collect()
}

pub(crate) fn parse_modules(
    root: &JsonObject,
) -> Result<Vec<topaz_kernel::CanonicalPreviewModule>, String> {
    json_array_field(root, "modules")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = exact_object(
                value,
                &format!("module {ordinal}"),
                &[
                    "sourceOrdinal",
                    "identity",
                    "path",
                    "source",
                    "entry",
                    "extern",
                    "generatedStd",
                    "raw",
                    "layout",
                    "ast",
                ],
            )?;
            let _ = json_i64(object, "sourceOrdinal")?;
            Ok(topaz_kernel::CanonicalPreviewModule {
                identity: json_string_field(object, "identity")?.to_string(),
                path: topaz_resolve::normalize_path(json_string_field(object, "path")?),
                source: json_string_field(object, "source")?.to_string(),
                entry: json_bool_field(object, "entry")?,
                extern_module: json_bool_field(object, "extern")?,
                generated_std: json_bool_field(object, "generatedStd")?,
                raw: parse_tokens(object, "raw", "raw")?,
                layout: parse_tokens(object, "layout", "layout")?,
                ast: parse_ast(object)?,
            })
        })
        .collect()
}

pub(crate) fn parse_edges(
    root: &JsonObject,
) -> Result<Vec<topaz_kernel::CanonicalPreviewImportEdge>, String> {
    json_array_field(root, "edges")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = exact_object(value, &format!("edge {ordinal}"), &["from", "to"])?;
            Ok(topaz_kernel::CanonicalPreviewImportEdge {
                from: json_string_field(object, "from")?.to_string(),
                to: json_string_field(object, "to")?.to_string(),
            })
        })
        .collect()
}

pub(crate) fn parse_scopes(
    root: &JsonObject,
) -> Result<Vec<topaz_kernel::CanonicalPreviewResolvedScope>, String> {
    json_array_field(root, "scopes")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = exact_object(
                value,
                &format!("scope {ordinal}"),
                &[
                    "moduleIndex",
                    "ordinal",
                    "parentOrdinal",
                    "kind",
                    "lo",
                    "hi",
                ],
            )?;
            let parent = json_i64(object, "parentOrdinal")?;
            Ok(topaz_kernel::CanonicalPreviewResolvedScope {
                module_index: usize::try_from(json_i64(object, "moduleIndex")?).map_err(|_| {
                    format!("front-end resolver scope {ordinal} module is negative")
                })?,
                ordinal: json_u32(object, "ordinal")?,
                parent_ordinal: if parent < 0 {
                    None
                } else {
                    Some(u32::try_from(parent).map_err(|_| {
                        format!("front-end resolver scope {ordinal} parent exceeds u32")
                    })?)
                },
                kind: json_string_field(object, "kind")?.to_string(),
                lo: json_u32(object, "lo")?,
                hi: json_u32(object, "hi")?,
            })
        })
        .collect()
}

pub(crate) fn optional_string(object: &JsonObject, field: &str) -> Result<Option<String>, String> {
    let value = json_string_field(object, field)?;
    Ok((!value.is_empty()).then(|| value.to_string()))
}

pub(crate) fn parse_declarations(
    root: &JsonObject,
) -> Result<Vec<topaz_kernel::CanonicalPreviewResolvedDeclaration>, String> {
    json_array_field(root, "declarations")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = exact_object(
                value,
                &format!("declaration {ordinal}"),
                &[
                    "moduleIndex",
                    "scopeOrdinal",
                    "name",
                    "namespace",
                    "declarationKind",
                    "lo",
                    "hi",
                    "exported",
                    "targetModule",
                    "targetName",
                ],
            )?;
            Ok(topaz_kernel::CanonicalPreviewResolvedDeclaration {
                module_index: usize::try_from(json_i64(object, "moduleIndex")?).map_err(|_| {
                    format!("front-end resolver declaration {ordinal} module is negative")
                })?,
                scope_ordinal: json_u32(object, "scopeOrdinal")?,
                name: json_string_field(object, "name")?.to_string(),
                namespace: json_string_field(object, "namespace")?.to_string(),
                declaration_kind: json_string_field(object, "declarationKind")?.to_string(),
                lo: json_u32(object, "lo")?,
                hi: json_u32(object, "hi")?,
                exported: json_bool_field(object, "exported")?,
                target_module: optional_string(object, "targetModule")?,
                target_name: optional_string(object, "targetName")?,
            })
        })
        .collect()
}

pub(crate) fn parse_references(
    root: &JsonObject,
) -> Result<Vec<topaz_kernel::CanonicalPreviewResolvedReference>, String> {
    json_array_field(root, "references")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = exact_object(
                value,
                &format!("reference {ordinal}"),
                &[
                    "moduleIndex",
                    "scopeOrdinal",
                    "name",
                    "namespace",
                    "role",
                    "lo",
                    "hi",
                    "targetModuleIndex",
                    "targetLo",
                    "targetHi",
                    "targetNamespace",
                    "targetModule",
                    "targetName",
                ],
            )?;
            let target_module_index = json_i64(object, "targetModuleIndex")?;
            Ok(topaz_kernel::CanonicalPreviewResolvedReference {
                module_index: usize::try_from(json_i64(object, "moduleIndex")?).map_err(|_| {
                    format!("front-end resolver reference {ordinal} module is negative")
                })?,
                scope_ordinal: json_u32(object, "scopeOrdinal")?,
                name: json_string_field(object, "name")?.to_string(),
                namespace: json_string_field(object, "namespace")?.to_string(),
                role: json_string_field(object, "role")?.to_string(),
                lo: json_u32(object, "lo")?,
                hi: json_u32(object, "hi")?,
                target_module_index: if target_module_index < 0 {
                    None
                } else {
                    Some(usize::try_from(target_module_index).map_err(|_| {
                        format!("front-end resolver reference {ordinal} target module is invalid")
                    })?)
                },
                target_lo: json_u32(object, "targetLo")?,
                target_hi: json_u32(object, "targetHi")?,
                target_namespace: optional_string(object, "targetNamespace")?,
                target_module: optional_string(object, "targetModule")?,
                target_name: optional_string(object, "targetName")?,
            })
        })
        .collect()
}

pub(crate) fn parse_exports(
    root: &JsonObject,
) -> Result<Vec<topaz_kernel::CanonicalPreviewResolvedExport>, String> {
    json_array_field(root, "exports")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = exact_object(
                value,
                &format!("export {ordinal}"),
                &[
                    "moduleIndex",
                    "name",
                    "namespace",
                    "declarationLo",
                    "declarationHi",
                ],
            )?;
            Ok(topaz_kernel::CanonicalPreviewResolvedExport {
                module_index: usize::try_from(json_i64(object, "moduleIndex")?).map_err(|_| {
                    format!("front-end resolver export {ordinal} module is negative")
                })?,
                name: json_string_field(object, "name")?.to_string(),
                namespace: json_string_field(object, "namespace")?.to_string(),
                declaration_lo: json_u32(object, "declarationLo")?,
                declaration_hi: json_u32(object, "declarationHi")?,
            })
        })
        .collect()
}

pub(crate) fn parse_resolved_diagnostics(
    root: &JsonObject,
) -> Result<Vec<topaz_kernel::CanonicalPreviewResolvedDiagnostic>, String> {
    parse_resolved_diagnostics_with(root, ResolvedDiagnosticShape::Current)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolvedDiagnosticShape {
    Current,
    SealedImage,
}

pub(crate) fn parse_resolved_diagnostics_with(
    root: &JsonObject,
    shape: ResolvedDiagnosticShape,
) -> Result<Vec<topaz_kernel::CanonicalPreviewResolvedDiagnostic>, String> {
    json_array_field(root, "resolverDiagnostics")?
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let object = resolved_diagnostic_object(value, ordinal, shape)?;
            Ok(topaz_kernel::CanonicalPreviewResolvedDiagnostic {
                module_index: usize::try_from(json_i64(object, "moduleIndex")?).map_err(|_| {
                    format!("front-end resolver diagnostic {ordinal} module is negative")
                })?,
                code: json_string_field(object, "code")?.to_string(),
                message: json_string_field(object, "message")?.to_string(),
                lo: json_u32(object, "lo")?,
                hi: json_u32(object, "hi")?,
                notes: if object.contains_key("notes") {
                    json_string_array_field(
                        object,
                        "notes",
                        &format!("resolver diagnostic {ordinal}"),
                    )?
                } else {
                    Vec::new()
                },
            })
        })
        .collect()
}

pub(crate) fn resolved_diagnostic_object(
    value: &JsonValue,
    ordinal: usize,
    shape: ResolvedDiagnosticShape,
) -> Result<&JsonObject, String> {
    const SEALED_FIELDS: &[&str] = &["moduleIndex", "code", "message", "lo", "hi"];
    const CURRENT_FIELDS: &[&str] = &["moduleIndex", "code", "message", "lo", "hi", "notes"];
    let fields = match value {
        JsonValue::Object(object)
            if shape == ResolvedDiagnosticShape::SealedImage && !object.contains_key("notes") =>
        {
            SEALED_FIELDS
        }
        _ => CURRENT_FIELDS,
    };
    exact_object(value, &format!("resolver diagnostic {ordinal}"), fields)
}

pub(crate) fn json_string_field<'a>(
    object: &'a std::collections::BTreeMap<std::rc::Rc<str>, JsonValue>,
    field: &str,
) -> Result<&'a str, String> {
    match object.get(field) {
        Some(JsonValue::String(value)) => Ok(value),
        _ => Err(format!("front-end preview `{field}` is not a string")),
    }
}

pub(crate) fn json_string_array_field(
    object: &JsonObject,
    field: &str,
    context: &str,
) -> Result<Vec<String>, String> {
    let Some(JsonValue::Array(values)) = object.get(field) else {
        return Err(format!("front-end {context} `{field}` is not an array"));
    };
    values
        .iter()
        .enumerate()
        .map(|(ordinal, value)| match value {
            JsonValue::String(value) => Ok(value.to_string()),
            _ => Err(format!(
                "front-end {context} `{field}` item {ordinal} is not a string"
            )),
        })
        .collect()
}

pub(crate) fn expect_json_string(
    object: &std::collections::BTreeMap<std::rc::Rc<str>, JsonValue>,
    field: &str,
    expected: &str,
) -> Result<(), String> {
    let actual = json_string_field(object, field)?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "front-end preview `{field}` is `{actual}`, expected `{expected}`"
        ))
    }
}

pub(crate) fn json_u32(
    object: &std::collections::BTreeMap<std::rc::Rc<str>, JsonValue>,
    field: &str,
) -> Result<u32, String> {
    let Some(JsonValue::Number(value)) = object.get(field) else {
        return Err(format!("front-end preview `{field}` is not a number"));
    };
    let value = value
        .int
        .ok_or_else(|| format!("front-end preview `{field}` is not an integer"))?;
    u32::try_from(value).map_err(|_| format!("front-end preview `{field}` is outside u32"))
}

pub(crate) fn json_i64(
    object: &std::collections::BTreeMap<std::rc::Rc<str>, JsonValue>,
    field: &str,
) -> Result<i64, String> {
    let Some(JsonValue::Number(value)) = object.get(field) else {
        return Err(format!("front-end preview `{field}` is not a number"));
    };
    value
        .int
        .ok_or_else(|| format!("front-end preview `{field}` is not an integer"))
}

pub(crate) fn parse_tokens(
    root: &std::collections::BTreeMap<std::rc::Rc<str>, JsonValue>,
    field: &str,
    expected_stream: &str,
) -> Result<Vec<topaz_kernel::CanonicalPreviewToken>, String> {
    let Some(JsonValue::Array(values)) = root.get(field) else {
        return Err(format!("front-end preview `{field}` is not an array"));
    };
    values
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let JsonValue::Object(object) = value else {
                return Err(format!(
                    "front-end preview {field} token {ordinal} is not an object"
                ));
            };
            let expected = ["hi", "kind", "lo", "stream", "synthetic"];
            if object.len() != expected.len()
                || !expected.iter().all(|key| object.contains_key(*key))
            {
                return Err(format!(
                    "front-end preview {field} token {ordinal} fields drifted"
                ));
            }
            expect_json_string(object, "stream", expected_stream)?;
            let synthetic = match object.get("synthetic") {
                Some(JsonValue::Bool(value)) => *value,
                _ => {
                    return Err(format!(
                        "front-end preview {field} token {ordinal} synthetic is not boolean"
                    ));
                }
            };
            Ok(topaz_kernel::CanonicalPreviewToken {
                kind: json_string_field(object, "kind")?.to_string(),
                lo: json_u32(object, "lo")?,
                hi: json_u32(object, "hi")?,
                synthetic,
            })
        })
        .collect()
}

pub(crate) fn parse_diagnostics(
    root: &std::collections::BTreeMap<std::rc::Rc<str>, JsonValue>,
) -> Result<Vec<topaz_kernel::CanonicalPreviewDiagnostic>, String> {
    let Some(JsonValue::Array(values)) = root.get("diagnostics") else {
        return Err("front-end preview `diagnostics` is not an array".to_string());
    };
    values
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let JsonValue::Object(object) = value else {
                return Err(format!(
                    "front-end preview diagnostic {ordinal} is not an object"
                ));
            };
            let expected = ["code", "hi", "lo", "message", "notes"];
            if object.len() != expected.len()
                || !expected.iter().all(|key| object.contains_key(*key))
            {
                return Err(format!(
                    "front-end preview diagnostic {ordinal} fields drifted"
                ));
            }
            Ok(topaz_kernel::CanonicalPreviewDiagnostic {
                code: json_string_field(object, "code")?.to_string(),
                message: json_string_field(object, "message")?.to_string(),
                lo: json_u32(object, "lo")?,
                hi: json_u32(object, "hi")?,
                notes: json_string_array_field(object, "notes", &format!("diagnostic {ordinal}"))?,
            })
        })
        .collect()
}

pub(crate) fn parse_ast(
    root: &std::collections::BTreeMap<std::rc::Rc<str>, JsonValue>,
) -> Result<Vec<topaz_kernel::CanonicalPreviewAstNode>, String> {
    let Some(JsonValue::Array(values)) = root.get("ast") else {
        return Err("front-end preview `ast` is not an array".to_string());
    };
    values
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let JsonValue::Object(object) = value else {
                return Err(format!(
                    "front-end preview AST node {ordinal} is not an object"
                ));
            };
            let expected = [
                "attributeKinds",
                "attributeNames",
                "attributeValues",
                "field",
                "hi",
                "index",
                "kind",
                "lo",
                "parent",
            ];
            if object.len() != expected.len()
                || !expected.iter().all(|key| object.contains_key(*key))
            {
                return Err(format!(
                    "front-end preview AST node {ordinal} fields drifted"
                ));
            }
            let arrays = ["attributeNames", "attributeKinds", "attributeValues"]
                .map(|field| {
                    let Some(JsonValue::Array(values)) = object.get(field) else {
                        return Err(format!(
                            "front-end preview AST node {ordinal} `{field}` is not an array"
                        ));
                    };
                    values
                        .iter()
                        .map(|value| match value {
                            JsonValue::String(value) => Ok(value.to_string()),
                            _ => Err(format!(
                                "front-end preview AST node {ordinal} `{field}` contains a non-string"
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .into_iter()
                .collect::<Result<Vec<_>, String>>()?;
            if arrays[0].len() != arrays[1].len() || arrays[0].len() != arrays[2].len() {
                return Err(format!(
                    "front-end preview AST node {ordinal} attribute columns differ in length"
                ));
            }
            let attributes = (0..arrays[0].len())
                .map(|index| {
                    let value = match arrays[1][index].as_str() {
                        "string" => {
                            topaz_kernel::CanonicalPreviewAstValue::String(arrays[2][index].clone())
                        }
                        "bool" if arrays[2][index] == "true" => {
                            topaz_kernel::CanonicalPreviewAstValue::Bool(true)
                        }
                        "bool" if arrays[2][index] == "false" => {
                            topaz_kernel::CanonicalPreviewAstValue::Bool(false)
                        }
                        "null" if arrays[2][index].is_empty() => {
                            topaz_kernel::CanonicalPreviewAstValue::Null
                        }
                        kind => {
                            return Err(format!(
                                "front-end preview AST node {ordinal} has invalid attribute kind/value `{kind}`"
                            ));
                        }
                    };
                    Ok(topaz_kernel::CanonicalPreviewAstAttribute {
                        name: arrays[0][index].clone(),
                        value,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let parent = json_i64(object, "parent")?;
            Ok(topaz_kernel::CanonicalPreviewAstNode {
                kind: json_string_field(object, "kind")?.to_string(),
                lo: json_u32(object, "lo")?,
                hi: json_u32(object, "hi")?,
                parent: if parent == -1 {
                    None
                } else {
                    Some(
                        u32::try_from(parent)
                            .map_err(|_| format!("AST node {ordinal} parent is outside u32"))?,
                    )
                },
                field: json_string_field(object, "field")?.to_string(),
                index: u64::try_from(json_i64(object, "index")?)
                    .map_err(|_| format!("AST node {ordinal} index is negative"))?,
                attributes,
            })
        })
        .collect()
}
