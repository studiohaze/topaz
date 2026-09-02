use super::{model::*, validate::*};
use crate::*;

pub(crate) fn object<'a>(
    value: &'a JsonValue,
    context: &str,
) -> Result<&'a BTreeMap<Rc<str>, JsonValue>, String> {
    match value {
        JsonValue::Object(value) => Ok(value),
        _ => Err(format!("{context} is not an object")),
    }
}

pub(crate) fn array<'a>(value: &'a JsonValue, context: &str) -> Result<&'a [JsonValue], String> {
    match value {
        JsonValue::Array(value) => Ok(value),
        _ => Err(format!("{context} is not an array")),
    }
}

pub(crate) fn field<'a>(
    value: &'a BTreeMap<Rc<str>, JsonValue>,
    name: &str,
    context: &str,
) -> Result<&'a JsonValue, String> {
    value
        .get(name)
        .ok_or_else(|| format!("{context} omitted `{name}`"))
}

pub(crate) fn string(
    value: &BTreeMap<Rc<str>, JsonValue>,
    name: &str,
    context: &str,
) -> Result<String, String> {
    match field(value, name, context)? {
        JsonValue::String(value) => Ok(value.to_string()),
        _ => Err(format!("{context}.{name} is not a string")),
    }
}

pub(crate) fn string_ref<'a>(
    value: &'a BTreeMap<Rc<str>, JsonValue>,
    name: &str,
    context: &str,
) -> Result<&'a str, String> {
    match field(value, name, context)? {
        JsonValue::String(value) => Ok(value),
        _ => Err(format!("{context}.{name} is not a string")),
    }
}

pub(crate) fn boolean(
    value: &BTreeMap<Rc<str>, JsonValue>,
    name: &str,
    context: &str,
) -> Result<bool, String> {
    match field(value, name, context)? {
        JsonValue::Bool(value) => Ok(*value),
        _ => Err(format!("{context}.{name} is not a boolean")),
    }
}

pub(crate) fn integer(
    value: &BTreeMap<Rc<str>, JsonValue>,
    name: &str,
    context: &str,
) -> Result<u32, String> {
    match field(value, name, context)? {
        JsonValue::Number(value) => value
            .lexeme
            .parse::<u32>()
            .map_err(|_| format!("{context}.{name} is not a u32")),
        _ => Err(format!("{context}.{name} is not a number")),
    }
}

pub(crate) fn signed_integer(
    value: &BTreeMap<Rc<str>, JsonValue>,
    name: &str,
    context: &str,
) -> Result<i64, String> {
    match field(value, name, context)? {
        JsonValue::Number(value) => value
            .lexeme
            .parse::<i64>()
            .map_err(|_| format!("{context}.{name} is not an i64")),
        _ => Err(format!("{context}.{name} is not a number")),
    }
}

#[derive(Debug)]
pub(crate) struct FlatRuntimeTypeAtom {
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

pub(crate) fn substitute_semantic_type(
    ty: &SemanticType,
    bindings: &BTreeMap<String, SemanticType>,
) -> SemanticType {
    match ty {
        SemanticType::Rigid { name, .. } if bindings.contains_key(name) => bindings[name].clone(),
        SemanticType::Union(values) => SemanticType::Union(
            values
                .iter()
                .map(|value| substitute_semantic_type(value, bindings))
                .collect(),
        ),
        SemanticType::Record(fields) => SemanticType::Record(
            fields
                .iter()
                .map(|field| SemanticField {
                    name: field.name.clone(),
                    ty: substitute_semantic_type(&field.ty, bindings),
                })
                .collect(),
        ),
        SemanticType::Constructor {
            constructor,
            arguments,
        } => SemanticType::Constructor {
            constructor: *constructor,
            arguments: arguments
                .iter()
                .map(|argument| substitute_semantic_type(argument, bindings))
                .collect(),
        },
        SemanticType::Function {
            parameters,
            variadic,
            result,
        } => SemanticType::Function {
            parameters: parameters
                .iter()
                .map(|parameter| substitute_semantic_type(parameter, bindings))
                .collect(),
            variadic: variadic
                .as_deref()
                .map(|value| Box::new(substitute_semantic_type(value, bindings))),
            result: Box::new(substitute_semantic_type(result, bindings)),
        },
        SemanticType::Foreign {
            identity,
            arguments,
        } => SemanticType::Foreign {
            identity: identity.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute_semantic_type(argument, bindings))
                .collect(),
        },
        SemanticType::Enum {
            identity,
            arguments,
        } => SemanticType::Enum {
            identity: identity.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute_semantic_type(argument, bindings))
                .collect(),
        },
        SemanticType::NominalRecord {
            identity,
            arguments,
        } => SemanticType::NominalRecord {
            identity: identity.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute_semantic_type(argument, bindings))
                .collect(),
        },
        SemanticType::Newtype {
            identity,
            arguments,
        } => SemanticType::Newtype {
            identity: identity.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute_semantic_type(argument, bindings))
                .collect(),
        },
        _ => ty.clone(),
    }
}

pub(crate) fn runtime_type_children<'a>(
    atoms: &'a [FlatRuntimeTypeAtom],
    parent: usize,
    field_name: &str,
) -> Result<Vec<(usize, &'a FlatRuntimeTypeAtom)>, String> {
    let mut children = atoms
        .iter()
        .enumerate()
        .filter(|(_, atom)| atom.parent == parent as i64 && atom.field == field_name)
        .collect::<Vec<_>>();
    children.sort_by_key(|(_, atom)| atom.index);
    if children
        .iter()
        .enumerate()
        .any(|(index, (_, atom))| atom.index != index)
    {
        return Err(format!(
            "typed pattern `{field_name}` child indices are not contiguous"
        ));
    }
    Ok(children)
}

pub(crate) fn runtime_semantic_type_at(
    atoms: &[FlatRuntimeTypeAtom],
    ordinal: usize,
) -> Result<SemanticType, String> {
    let atom = atoms
        .get(ordinal)
        .ok_or_else(|| "typed pattern type ordinal is outside the tree".to_string())?;
    let nested = |field_name: &str| -> Result<Vec<SemanticType>, String> {
        runtime_type_children(atoms, ordinal, field_name)?
            .into_iter()
            .map(|(index, _)| runtime_semantic_type_at(atoms, index))
            .collect()
    };
    Ok(match atom.kind.as_str() {
        "primitive" => SemanticType::Primitive(match atom.name.as_str() {
            "int" => SemanticPrimitive::Int,
            "float" => SemanticPrimitive::Float,
            "string" => SemanticPrimitive::String,
            "bool" => SemanticPrimitive::Bool,
            "unit" => SemanticPrimitive::Unit,
            name => return Err(format!("typed pattern has unknown primitive `{name}`")),
        }),
        "literal" => SemanticType::Literal(match atom.name.as_str() {
            "string" => SemanticLiteral::String(atom.value.clone()),
            "int" => SemanticLiteral::Int(
                atom.value
                    .parse()
                    .map_err(|_| "typed pattern integer literal is invalid".to_string())?,
            ),
            "float" => SemanticLiteral::Float(atom.value.clone()),
            "bool" if atom.value == "true" => SemanticLiteral::Bool(true),
            "bool" if atom.value == "false" => SemanticLiteral::Bool(false),
            "null" => SemanticLiteral::Null,
            name => return Err(format!("typed pattern has unknown literal `{name}`")),
        }),
        "union" => SemanticType::Union(nested("members")?),
        "record" => SemanticType::Record(
            runtime_type_children(atoms, ordinal, "fields")?
                .into_iter()
                .map(|(index, atom)| {
                    Ok(SemanticField {
                        name: atom.edge_name.clone(),
                        ty: runtime_semantic_type_at(atoms, index)?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        ),
        "constructor" => SemanticType::Constructor {
            constructor: match atom.name.as_str() {
                "Array" => SemanticConstructor::Array,
                "Map" => SemanticConstructor::Map,
                "Set" => SemanticConstructor::Set,
                "Option" => SemanticConstructor::Option,
                "Result" => SemanticConstructor::Result,
                "Range" => SemanticConstructor::Range,
                name => {
                    return Err(format!(
                        "typed pattern has unknown semantic constructor `{name}`"
                    ));
                }
            },
            arguments: nested("arguments")?,
        },
        "function" => {
            let mut result = nested("result")?;
            if result.len() != 1 {
                return Err("typed pattern function type needs one result".to_string());
            }
            let variadic = nested("variadic")?;
            if variadic.len() > 1 {
                return Err("typed pattern function type has multiple variadic types".to_string());
            }
            SemanticType::Function {
                parameters: nested("parameters")?,
                variadic: variadic.into_iter().next().map(Box::new),
                result: Box::new(result.remove(0)),
            }
        }
        "foreign" => SemanticType::Foreign {
            identity: atom.identity.clone(),
            arguments: nested("arguments")?,
        },
        "rigid" => SemanticType::Rigid {
            name: atom.name.clone(),
            _origin: atom.origin.clone(),
        },
        "enum" => SemanticType::Enum {
            identity: atom.identity.clone(),
            arguments: nested("arguments")?,
        },
        "nominal-record" => SemanticType::NominalRecord {
            identity: atom.identity.clone(),
            arguments: nested("arguments")?,
        },
        "newtype" => SemanticType::Newtype {
            identity: atom.identity.clone(),
            arguments: nested("arguments")?,
        },
        "template" => SemanticType::Template,
        "file" => SemanticType::File,
        "json-value" => SemanticType::JsonValue,
        "bytes" => SemanticType::Bytes,
        "byte-buffer" => SemanticType::ByteBuffer,
        "path" => SemanticType::Path,
        "regex" => SemanticType::Regex,
        "match" => SemanticType::Match,
        "toml-value" => SemanticType::TomlValue,
        "url" => SemanticType::Url,
        "date" => SemanticType::Date,
        "big-int" => SemanticType::BigInt,
        "decimal" => SemanticType::Decimal,
        "rounding-mode" => SemanticType::RoundingMode,
        "inference-variable" => SemanticType::InferenceVariable,
        "unknown" => SemanticType::Unknown,
        kind => return Err(format!("typed pattern has unknown type kind `{kind}`")),
    })
}

pub(crate) fn parse_runtime_type(encoded: &str, context: &str) -> Result<SemanticType, String> {
    if !encoded.starts_with('[') {
        return Err(format!("{context} has no runtime type descriptor"));
    }
    let parsed = topaz_value::value::json_parse(encoded)
        .map_err(|error| format!("{context} runtime type is invalid JSON: {error:?}"))?;
    let values = array(&parsed, &format!("{context} runtime type"))?;
    if values.is_empty() {
        return Err(format!("{context} runtime type is empty"));
    }
    let mut atoms: Vec<FlatRuntimeTypeAtom> = Vec::with_capacity(values.len());
    for (ordinal, value) in values.iter().enumerate() {
        let atom_context = format!("{context} runtime type atom {ordinal}");
        let object = object(value, &atom_context)?;
        const FIELDS: &[&str] = &[
            "parent", "field", "index", "edgeName", "kind", "name", "value", "identity", "origin",
        ];
        if object.len() != FIELDS.len() || FIELDS.iter().any(|name| !object.contains_key(*name)) {
            return Err(format!("{atom_context} has an invalid field set"));
        }
        let parent = signed_integer(object, "parent", &atom_context)?;
        let atom = FlatRuntimeTypeAtom {
            parent,
            field: string(object, "field", &atom_context)?,
            index: usize::try_from(integer(object, "index", &atom_context)?)
                .map_err(|_| format!("{atom_context}.index is too large"))?,
            edge_name: string(object, "edgeName", &atom_context)?,
            kind: string(object, "kind", &atom_context)?,
            name: string(object, "name", &atom_context)?,
            value: string(object, "value", &atom_context)?,
            identity: string(object, "identity", &atom_context)?,
            origin: string(object, "origin", &atom_context)?,
        };
        if ordinal == 0 {
            if atom.parent != -1
                || atom.field != "root"
                || atom.index != 0
                || !atom.edge_name.is_empty()
            {
                return Err(format!("{atom_context} is not the exact root atom"));
            }
        } else {
            let parent = usize::try_from(atom.parent)
                .ok()
                .filter(|parent| *parent < ordinal)
                .ok_or_else(|| format!("{atom_context}.parent does not precede its child"))?;
            let parent_kind = atoms[parent].kind.as_str();
            let valid_field = match parent_kind {
                "union" => atom.field == "members",
                "record" => atom.field == "fields",
                "constructor" | "foreign" | "enum" | "nominal-record" | "newtype" => {
                    atom.field == "arguments"
                }
                "function" => matches!(atom.field.as_str(), "parameters" | "variadic" | "result"),
                _ => false,
            };
            if !valid_field {
                return Err(format!(
                    "{atom_context}.field `{}` is invalid for parent kind `{parent_kind}`",
                    atom.field
                ));
            }
            if (atom.field == "fields") == atom.edge_name.is_empty() {
                return Err(format!(
                    "{atom_context}.edgeName does not match its field role"
                ));
            }
        }
        atoms.push(atom);
    }
    runtime_semantic_type_at(&atoms, 0)
}

pub(crate) fn strings(
    value: &BTreeMap<Rc<str>, JsonValue>,
    name: &str,
    context: &str,
) -> Result<Vec<String>, String> {
    array(field(value, name, context)?, &format!("{context}.{name}"))?
        .iter()
        .enumerate()
        .map(|(index, value)| match value {
            JsonValue::String(value) => Ok(value.to_string()),
            _ => Err(format!("{context}.{name}[{index}] is not a string")),
        })
        .collect()
}

pub(crate) fn call_arguments(
    value: &BTreeMap<Rc<str>, JsonValue>,
    context: &str,
) -> Result<Vec<CallArgument>, String> {
    strings(value, "callArguments", context)?
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let mut fields = row.split('|');
            let kind = fields.next().unwrap_or_default();
            let name = fields.next().unwrap_or_default();
            let source_index = fields
                .next()
                .ok_or_else(|| format!("{context}.callArguments[{index}] is truncated"))?
                .parse::<i64>()
                .map_err(|_| {
                    format!("{context}.callArguments[{index}] source index is not an integer")
                })?;
            let lo = fields
                .next()
                .ok_or_else(|| format!("{context}.callArguments[{index}] is truncated"))?
                .parse::<u32>()
                .map_err(|_| format!("{context}.callArguments[{index}] lo is not a u32"))?;
            let hi = fields
                .next()
                .ok_or_else(|| format!("{context}.callArguments[{index}] is truncated"))?
                .parse::<u32>()
                .map_err(|_| format!("{context}.callArguments[{index}] hi is not a u32"))?;
            if fields.next().is_some() || hi < lo {
                return Err(format!(
                    "{context}.callArguments[{index}] has an invalid exact row"
                ));
            }
            let source_index = match source_index {
                -1 => None,
                value if value >= 0 => Some(usize::try_from(value).map_err(|_| {
                    format!("{context}.callArguments[{index}] source index is out of range")
                })?),
                _ => {
                    return Err(format!(
                        "{context}.callArguments[{index}] source index is below -1"
                    ));
                }
            };
            let binding = match kind {
                "positional" if name.is_empty() && source_index.is_some() => {
                    CallArgumentBinding::Positional
                }
                "named" if !name.is_empty() && source_index.is_some() => {
                    CallArgumentBinding::Named(name.to_string())
                }
                "spread" if name.is_empty() && source_index.is_some() => {
                    CallArgumentBinding::Spread
                }
                "inserted-lead" if name.is_empty() => CallArgumentBinding::InsertedLead,
                _ => {
                    return Err(format!(
                        "{context}.callArguments[{index}] has invalid binding `{kind}`"
                    ));
                }
            };
            Ok(CallArgument {
                binding,
                source_index,
                lo,
                hi,
            })
        })
        .collect()
}

pub(crate) fn call_evaluations(
    value: &BTreeMap<Rc<str>, JsonValue>,
    context: &str,
) -> Result<Vec<CallEvaluation>, String> {
    strings(value, "callEvaluations", context)?
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let mut fields = row.split('|');
            let kind = fields.next().unwrap_or_default();
            let argument_index = fields
                .next()
                .ok_or_else(|| format!("{context}.callEvaluations[{index}] is truncated"))?
                .parse::<i64>()
                .map_err(|_| {
                    format!("{context}.callEvaluations[{index}] argument index is not an integer")
                })?;
            if fields.next().is_some() {
                return Err(format!(
                    "{context}.callEvaluations[{index}] has an invalid exact row"
                ));
            }
            match (kind, argument_index) {
                ("callee", -1) => Ok(CallEvaluation::Callee),
                ("receiver", -1) => Ok(CallEvaluation::Receiver),
                ("optional-guard", -1) => Ok(CallEvaluation::OptionalGuard),
                ("pipe-lead", -1) => Ok(CallEvaluation::PipeLead),
                ("argument", value) if value >= 0 => Ok(CallEvaluation::Argument(
                    usize::try_from(value).map_err(|_| {
                        format!("{context}.callEvaluations[{index}] argument index is out of range")
                    })?,
                )),
                _ => Err(format!(
                    "{context}.callEvaluations[{index}] has invalid evaluation `{kind}`"
                )),
            }
        })
        .collect()
}

pub(crate) fn indexed_string_references(
    value: &BTreeMap<Rc<str>, JsonValue>,
    name: &str,
    context: &str,
    indexes: &BTreeMap<&str, usize>,
    reference_kind: &str,
) -> Result<Vec<usize>, String> {
    array(field(value, name, context)?, &format!("{context}.{name}"))?
        .iter()
        .enumerate()
        .map(|(index, value)| match value {
            JsonValue::String(value) => indexes
                .get(value.as_ref())
                .copied()
                .ok_or_else(|| format!("{context} references unknown {reference_kind} `{value}`")),
            _ => Err(format!("{context}.{name}[{index}] is not a string")),
        })
        .collect()
}

pub(crate) fn parse_program(
    payload: &str,
    admission: ProgramAdmission,
) -> Result<ParsedProgram, String> {
    let parsed = topaz_value::value::json_parse(payload)
        .map_err(|error| format!("Stage 1 IR JSON is invalid: {error:?}"))?;
    let root = object(&parsed, "Stage 1 IR payload")?;
    if string(root, "schema", "Stage 1 IR payload")? != FIXED_POINT_PAYLOAD_SCHEMA {
        return Err("Stage 1 IR payload schema mismatch".to_string());
    }

    let operation_rows = array(
        field(root, "loweredOperations", "Stage 1 IR payload")?,
        "Stage 1 IR operations",
    )?;
    let mut indexes = BTreeMap::new();
    for (index, row) in operation_rows.iter().enumerate() {
        let row = object(row, &format!("Stage 1 IR operation {index}"))?;
        let id = string_ref(row, "id", &format!("Stage 1 IR operation {index}"))?;
        if indexes.insert(id, index).is_some() {
            return Err(format!("Stage 1 IR operation {index} duplicates an id"));
        }
    }

    let mut operations = Vec::with_capacity(operation_rows.len());
    let mut requires_host = false;
    for (index, row) in operation_rows.iter().enumerate() {
        let context = format!("Stage 1 IR operation {index}");
        let row = object(row, &context)?;
        let operands = indexed_string_references(row, "operands", &context, &indexes, "operand")?;
        let operand_labels = strings(row, "operandLabels", &context)?;
        if operands.len() != operand_labels.len() {
            return Err(format!("{context} operand labels are misaligned"));
        }
        let _binding_mutable = boolean(row, "bindingMutable", &context)?;
        let kind = string(row, "kind", &context)?;
        let semantic_type = string(row, "semanticType", &context)?;
        let pattern_type = if kind == "pattern/typed-binding" {
            semantic_type
                .starts_with('[')
                .then(|| parse_runtime_type(&semantic_type, &context))
                .transpose()?
        } else {
            None
        };
        let operation = Operation {
            id: string(row, "id", &context)?,
            module: string(row, "module", &context)?,
            lo: integer(row, "lo", &context)?,
            hi: integer(row, "hi", &context)?,
            kind,
            detail: string(row, "detail", &context)?,
            operands,
            operand_labels,
            semantic_type,
            pattern_type,
            reference_identity: string(row, "referenceIdentity", &context)?,
            binding_name: string(row, "bindingName", &context)?,
            declaration_identity: string(row, "declarationIdentity", &context)?,
            control_target: string(row, "controlTarget", &context)?,
            call_target: string(row, "callTarget", &context)?,
            call_callee_kind: string(row, "callCalleeKind", &context)?,
            call_method: string(row, "callMethod", &context)?,
            call_optional: boolean(row, "callOptional", &context)?,
            call_shadow_first: boolean(row, "callShadowFirst", &context)?,
            call_stage_method: string(row, "callStageMethod", &context)?,
            call_arguments: call_arguments(row, &context)?,
            call_evaluations: call_evaluations(row, &context)?,
        };
        validate_operation_shape(&operation, "Stage 1 IR", true)?;
        requires_host |= operation_requires_host(&operation);
        operations.push(operation);
    }
    let pipeline_stages = operations
        .iter()
        .filter(|operation| operation.kind == "expression/pipeline")
        .filter_map(|operation| operation.operands.get(1).copied())
        .collect::<BTreeSet<_>>();
    // Compiler images were sealed after exact call plans became mandatory.
    // The same v1 table schema also covers earlier target products, whose
    // completely metadata-free direct calls use the retained legacy evaluator.
    // Partial call metadata is rejected above for both admissions.
    if admission == ProgramAdmission::CompilerImage {
        for (index, operation) in operations.iter().enumerate() {
            if operation.kind == "expression/call"
                && operation.call_evaluations.is_empty()
                && !pipeline_stages.contains(&index)
            {
                return Err(format!(
                    "Stage 1 IR operation `{}` has no owning pipeline call plan",
                    operation.id
                ));
            }
        }
    }

    let module_rows = array(
        field(root, "loweredModules", "Stage 1 IR payload")?,
        "Stage 1 IR modules",
    )?;
    let mut modules = Vec::with_capacity(module_rows.len());
    for (index, row) in module_rows.iter().enumerate() {
        let context = format!("Stage 1 IR module {index}");
        let row = object(row, &context)?;
        let operations =
            indexed_string_references(row, "operationIds", &context, &indexes, "operation")?;
        modules.push(Module {
            identity: string(row, "identity", &context)?,
            entry: boolean(row, "entry", &context)?,
            operations,
        });
    }
    let program = Program {
        modules,
        operations,
    };
    Ok(ParsedProgram {
        program,
        requires_host,
    })
}
