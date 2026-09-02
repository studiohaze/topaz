use super::super::*;
use super::schema::*;

pub(super) fn validate_cross_references(
    files: &BTreeMap<&str, &ObservationFile>,
) -> Result<(), String> {
    let source_values = crate::canonical::validate(&files["source-set.jsonl"].bytes, true)?;
    let source_ids = source_values
        .iter()
        .filter(|value| string_field(value, "rowKind").is_ok_and(|kind| kind == "source"))
        .map(|value| string_field(value, "sourceId").map(str::to_string))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let source_members = source_values
        .iter()
        .filter(|value| string_field(value, "rowKind").is_ok_and(|kind| kind == "source"))
        .map(|value| string_field(value, "member").map(str::to_string))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if !source_members
        .iter()
        .all(|member| files.contains_key(member.as_str()))
    {
        return Err("source-set references a missing source member".to_string());
    }
    for path in ["tokens.jsonl", "ast.jsonl"] {
        if files[path].bytes.is_empty() {
            continue;
        }
        for value in crate::canonical::validate(&files[path].bytes, true)? {
            if !source_ids.contains(string_field(&value, "sourceId")?) {
                return Err(format!("`{path}` references an unknown source"));
            }
        }
    }
    let ast_values = if files["ast.jsonl"].bytes.is_empty() {
        Vec::new()
    } else {
        crate::canonical::validate(&files["ast.jsonl"].bytes, true)?
    };
    let node_ids = ast_values
        .iter()
        .map(|value| string_field(value, "nodeId").map(str::to_string))
        .collect::<Result<BTreeSet<_>, _>>()?;
    for value in &ast_values {
        if let Some(JsonValue::String(parent)) = object_fields(value)?.get("parentNodeId")
            && !node_ids.contains(parent.as_ref())
        {
            return Err("AST row references an unknown parent node".to_string());
        }
    }
    if let Some(typed) = files.get("typed.jsonl")
        && !typed.bytes.is_empty()
    {
        for value in crate::canonical::validate(&typed.bytes, true)? {
            if !source_ids.contains(string_field(&value, "sourceId")?) {
                return Err("typed row references an unknown source".to_string());
            }
            for field in match string_field(&value, "rowKind")? {
                "node" => &["nodeId"][..],
                "call" => &["callNodeId", "calleeNodeId"][..],
                "capture" => &["closureNodeId", "declarationNodeId", "referenceNodeId"][..],
                _ => unreachable!("schema validation checked row kinds"),
            } {
                if let Some(JsonValue::String(node)) = object_fields(&value)?.get(*field)
                    && !node_ids.contains(node.as_ref())
                {
                    return Err("typed row references an unknown AST node".to_string());
                }
            }
        }
    }
    if let Some(lowered) = files.get("lowered.jsonl")
        && !lowered.bytes.is_empty()
    {
        let values = crate::canonical::validate(&lowered.bytes, true)?;
        let operation_ids = values
            .iter()
            .filter(|value| string_field(value, "rowKind").is_ok_and(|kind| kind == "operation"))
            .map(|value| string_field(value, "operationId").map(str::to_string))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let module_ids = values
            .iter()
            .filter(|value| string_field(value, "rowKind").is_ok_and(|kind| kind == "module"))
            .map(|value| string_field(value, "identity").map(str::to_string))
            .collect::<Result<BTreeSet<_>, _>>()?;
        for value in &values {
            match string_field(value, "rowKind")? {
                "module" => {
                    if !source_ids.contains(string_field(value, "sourceId")?) {
                        return Err("lowered module references an unknown source".to_string());
                    }
                    for operation in array_field(value, "operationIds")? {
                        let JsonValue::String(operation) = operation else {
                            return Err(
                                "lowered module operation identity must be a string".to_string()
                            );
                        };
                        if !operation_ids.contains(operation.as_ref()) {
                            return Err(
                                "lowered module references an unknown operation".to_string()
                            );
                        }
                    }
                }
                "operation" => {
                    if !module_ids.contains(string_field(value, "module")?) {
                        return Err("lowered operation references an unknown module".to_string());
                    }
                    for field in ["parentOperationId"] {
                        if let JsonValue::String(operation) = value_field(value, field)?
                            && !operation_ids.contains(operation.as_ref())
                        {
                            return Err(format!(
                                "lowered operation `{}` references unknown parent `{operation}`",
                                string_field(value, "operationId")?
                            ));
                        }
                    }
                    for operation in array_field(value, "operands")? {
                        let JsonValue::String(operation) = operation else {
                            return Err("lowered operand identity must be a string".to_string());
                        };
                        if !operation_ids.contains(operation.as_ref()) {
                            return Err(
                                "lowered operation references an unknown operand".to_string()
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let resolved_values = if files["resolved.jsonl"].bytes.is_empty() {
        Vec::new()
    } else {
        crate::canonical::validate(&files["resolved.jsonl"].bytes, true)?
    };
    let scope_ids = resolved_values
        .iter()
        .filter(|value| string_field(value, "rowKind").is_ok_and(|kind| kind == "scope"))
        .map(|value| string_field(value, "scopeId").map(str::to_string))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let symbol_ids = resolved_values
        .iter()
        .filter(|value| string_field(value, "rowKind").is_ok_and(|kind| kind == "declaration"))
        .map(|value| {
            let Some(JsonValue::String(symbol)) = object_fields(value)?.get("symbolId") else {
                return Err("resolved declaration lacks a stable symbol identity".to_string());
            };
            Ok(symbol.to_string())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    for value in &resolved_values {
        let kind = string_field(value, "rowKind")?;
        if kind != "import-edge" && !source_ids.contains(string_field(value, "sourceId")?) {
            return Err("resolved row references an unknown source".to_string());
        }
        match kind {
            "scope" => {
                let Some(JsonValue::String(owner)) = object_fields(value)?.get("ownerNodeId")
                else {
                    return Err("resolved scope lacks an owner node".to_string());
                };
                if !node_ids.contains(owner.as_ref()) {
                    return Err("resolved scope references an unknown owner node".to_string());
                }
                if let Some(JsonValue::String(parent)) = object_fields(value)?.get("parentScopeId")
                    && !scope_ids.contains(parent.as_ref())
                {
                    return Err("resolved scope references an unknown parent scope".to_string());
                }
            }
            "declaration" => {
                if !scope_ids.contains(string_field(value, "scopeId")?) {
                    return Err("resolved declaration references an unknown scope".to_string());
                }
                let Some(JsonValue::String(node)) = object_fields(value)?.get("declarationNodeId")
                else {
                    return Err("resolved declaration lacks a declaration node".to_string());
                };
                if !node_ids.contains(node.as_ref()) {
                    return Err("resolved declaration references an unknown AST node".to_string());
                }
            }
            "reference" => {
                if !scope_ids.contains(string_field(value, "scopeId")?) {
                    return Err("resolved reference references an unknown scope".to_string());
                }
                let Some(JsonValue::String(node)) = object_fields(value)?.get("referenceNodeId")
                else {
                    return Err("resolved reference lacks a reference node".to_string());
                };
                if !node_ids.contains(node.as_ref()) {
                    return Err("resolved reference references an unknown AST node".to_string());
                }
                if let Some(JsonValue::String(symbol)) = object_fields(value)?.get("targetSymbolId")
                    && !symbol_ids.contains(symbol.as_ref())
                {
                    return Err("resolved reference targets an unknown symbol".to_string());
                }
            }
            "export" => {
                let Some(JsonValue::String(symbol)) = object_fields(value)?.get("symbolId") else {
                    return Err("resolved export lacks a stable symbol identity".to_string());
                };
                if !symbol_ids.contains(symbol.as_ref()) {
                    return Err("resolved export references an unknown symbol".to_string());
                }
            }
            "module" | "import-edge" => {}
            _ => unreachable!("schema validation checked row kinds"),
        }
    }
    Ok(())
}
