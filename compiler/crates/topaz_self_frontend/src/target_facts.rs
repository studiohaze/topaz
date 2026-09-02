use crate::*;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Parameter names and defaults come from checked declarations, not generated Rust parsing.
pub struct SelfTargetExportFact {
    pub name: String,
    pub ty: topaz_hir::SemanticType,
    pub type_parameters: usize,
    pub parameter_names: Vec<String>,
    pub parameter_defaults: Vec<bool>,
    pub names_known: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Default operation IDs link runtime materialization back to lowered code.
pub struct SelfTargetNominalMemberFact {
    pub name: String,
    pub arity: usize,
    pub default_operation_id: Option<String>,
    pub types: Vec<topaz_hir::SemanticType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Carries defining-module identity across the self compiler's private IR boundary.
pub struct SelfTargetNominalFact {
    pub name: String,
    pub identity: String,
    pub kind: String,
    pub type_parameters: Vec<String>,
    pub members: Vec<SelfTargetNominalMemberFact>,
    pub base_type: Option<topaz_hir::SemanticType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Lets runtime type operations avoid rediscovering nominal ownership from names.
pub struct SelfTargetOperationNominalFact {
    pub operation_id: String,
    pub identity: String,
    pub kind: String,
}

/// Target-neutral facts mechanically projected from a validated C2 product.
///
/// This is intentionally not another checker surface. It can only match
/// already-resolved exports to already-typed declaration nodes and already-
/// lowered function bindings. Missing or contradictory rows fail closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfTargetAdapterFacts {
    pub schema: &'static str,
    pub producer: &'static str,
    pub source_set_id: String,
    pub result_id: String,
    pub entry_module: String,
    pub exports: Vec<SelfTargetExportFact>,
    pub nominals: Vec<SelfTargetNominalFact>,
    pub operation_nominals: Vec<SelfTargetOperationNominalFact>,
    pub runtime_requirements: Vec<String>,
}

impl SelfTargetAdapterFacts {
    pub fn entry_function_exports(&self) -> impl Iterator<Item = &str> {
        self.exports
            .iter()
            .filter(|export| export.names_known)
            .map(|export| export.name.as_str())
    }

    pub fn has_explicit_main(&self) -> bool {
        self.entry_function_exports().any(|name| name == "main")
    }
}

/// Adapter facts paired with the borrowed IR consumed by one target execution.
pub struct SelfTargetRuntimeInputs<'a> {
    pub facts: SelfTargetAdapterFacts,
    pub ir_json: &'a str,
}

pub(crate) struct TargetAstIndex<'a> {
    module: &'a topaz_kernel::CanonicalPreviewModule,
    children: std::collections::HashMap<usize, std::collections::HashMap<&'a str, Vec<usize>>>,
    name_nodes: std::collections::HashMap<(u32, u32), Vec<usize>>,
}

impl<'a> TargetAstIndex<'a> {
    fn new(module: &'a topaz_kernel::CanonicalPreviewModule) -> Self {
        let mut children: std::collections::HashMap<
            usize,
            std::collections::HashMap<&'a str, Vec<usize>>,
        > = std::collections::HashMap::new();
        let mut name_nodes = std::collections::HashMap::new();
        for (index, node) in module.ast.iter().enumerate() {
            if let Some(parent) = node.parent {
                children
                    .entry(parent as usize)
                    .or_default()
                    .entry(node.field.as_str())
                    .or_default()
                    .push(index);
            }
            if node.kind == "identifier" && node.field == "name" {
                name_nodes
                    .entry((node.lo, node.hi))
                    .or_insert_with(Vec::new)
                    .push(index);
            }
        }
        Self {
            module,
            children,
            name_nodes,
        }
    }

    fn child_indices(&self, parent: usize, field: &str) -> &[usize] {
        self.children
            .get(&parent)
            .and_then(|children| children.get(field))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn name_indices(&self, lo: u32, hi: u32) -> &[usize] {
        self.name_nodes
            .get(&(lo, hi))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

pub(crate) fn ast_node_spelling(
    module: &topaz_kernel::CanonicalPreviewModule,
    index: usize,
    context: &str,
) -> Result<String, String> {
    let node = module
        .ast
        .get(index)
        .ok_or_else(|| format!("{context} is outside the module AST"))?;
    let spelling = node
        .attributes
        .iter()
        .find_map(|attribute| (attribute.name == "spelling").then_some(&attribute.value))
        .and_then(|value| match value {
            topaz_kernel::CanonicalPreviewAstValue::String(value) => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| module.source[node.lo as usize..node.hi as usize].to_string());
    if spelling.is_empty() {
        return Err(format!("{context} has an empty spelling"));
    }
    Ok(spelling)
}

pub(crate) type TargetTypeFactIndex<'a> = std::collections::HashMap<
    &'a str,
    std::collections::HashMap<(u32, u32), &'a topaz_hir::SemanticType>,
>;

pub(crate) fn target_type_fact(
    module: &topaz_kernel::CanonicalPreviewModule,
    type_facts: &TargetTypeFactIndex<'_>,
    index: usize,
    context: &str,
) -> Result<topaz_hir::SemanticType, String> {
    let node = module
        .ast
        .get(index)
        .ok_or_else(|| format!("{context} type node is outside the module AST"))?;
    let ty = type_facts
        .get(module.identity.as_str())
        .and_then(|facts| facts.get(&(node.lo, node.hi)))
        .ok_or_else(|| format!("{context} has no exact semantic type fact"))?;
    if ty.has_hole() {
        return Err(format!("{context} has an incomplete semantic type"));
    }
    Ok((*ty).clone())
}

pub(crate) fn target_nominal_fact(
    ast: &TargetAstIndex<'_>,
    type_facts: &TargetTypeFactIndex<'_>,
    has_type_facts: bool,
    operations: &[Stage1LoweredOperation],
    declaration_name: &str,
    name_lo: u32,
    name_hi: u32,
) -> Result<SelfTargetNominalFact, String> {
    let module = ast.module;
    let name_nodes = ast.name_indices(name_lo, name_hi);
    if name_nodes.len() != 1 {
        return Err(format!(
            "self target nominal `{declaration_name}` has {} exact AST names",
            name_nodes.len()
        ));
    }
    let declaration_index = module.ast[name_nodes[0]]
        .parent
        .map(|value| value as usize)
        .ok_or_else(|| {
            format!("self target nominal `{declaration_name}` has no AST declaration")
        })?;
    let declaration = module.ast.get(declaration_index).ok_or_else(|| {
        format!("self target nominal `{declaration_name}` AST declaration is out of range")
    })?;
    let (kind, member_field) = match declaration.kind.as_str() {
        "statement/record" => ("record", "fields"),
        "statement/enum" => ("enum", "variants"),
        "statement/newtype" => ("newtype", ""),
        other => {
            return Err(format!(
                "self target nominal `{declaration_name}` has unsupported AST kind `{other}`"
            ));
        }
    };
    let mut type_parameter_indices = ast
        .child_indices(declaration_index, "typeParameters")
        .to_vec();
    type_parameter_indices.sort_by_key(|index| module.ast[*index].index);
    let type_parameters = type_parameter_indices
        .into_iter()
        .map(|index| {
            ast_node_spelling(
                module,
                index,
                &format!("self target nominal `{declaration_name}` type parameter"),
            )
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut member_indices = if member_field.is_empty() {
        Vec::new()
    } else {
        ast.child_indices(declaration_index, member_field).to_vec()
    };
    member_indices.sort_by_key(|index| module.ast[*index].index);
    let mut members = Vec::with_capacity(member_indices.len());
    for member_index in member_indices {
        let name_children = ast.child_indices(member_index, "name");
        if name_children.len() != 1 {
            return Err(format!(
                "self target nominal `{declaration_name}` member has {} names",
                name_children.len()
            ));
        }
        let name = ast_node_spelling(
            module,
            name_children[0],
            &format!("self target nominal `{declaration_name}` member"),
        )?;
        let arity = if kind == "enum" {
            ast.child_indices(member_index, "payload").len()
        } else {
            1
        };
        let type_field = if kind == "enum" { "payload" } else { "type" };
        let mut type_indices = ast.child_indices(member_index, type_field).to_vec();
        type_indices.sort_by_key(|index| module.ast[*index].index);
        let types = if has_type_facts {
            type_indices
                .into_iter()
                .enumerate()
                .map(|(index, type_index)| {
                    target_type_fact(
                        module,
                        type_facts,
                        type_index,
                        &format!(
                            "self target nominal `{declaration_name}` member `{name}` type {index}"
                        ),
                    )
                })
                .collect::<Result<Vec<_>, String>>()?
        } else {
            Vec::new()
        };
        if has_type_facts && types.len() != arity {
            return Err(format!(
                "self target nominal `{declaration_name}` member `{name}` has {} semantic types for arity {arity}",
                types.len()
            ));
        }
        let default_indices = ast.child_indices(member_index, "default");
        if default_indices.len() > 1 {
            return Err(format!(
                "self target nominal `{declaration_name}` member `{name}` has {} defaults",
                default_indices.len()
            ));
        }
        let default_operation_id = default_indices
            .first()
            .map(|index| {
                let default = &module.ast[*index];
                let matches = operations
                    .iter()
                    .filter(|operation| {
                        operation.module == module.identity
                            && operation.lo == default.lo
                            && operation.hi == default.hi
                            && operation.role == "expression"
                    })
                    .collect::<Vec<_>>();
                if matches.len() != 1 {
                    return Err(format!(
                        "self target record `{declaration_name}` member `{name}` default has {} exact lowered operations",
                        matches.len()
                    ));
                }
                Ok(matches[0].id.clone())
            })
            .transpose()?;
        members.push(SelfTargetNominalMemberFact {
            name,
            arity,
            default_operation_id,
            types,
        });
    }
    if kind == "record" && members.is_empty() {
        return Err(format!(
            "self target record `{declaration_name}` has no projected fields"
        ));
    }
    let base_type = if kind == "newtype" && has_type_facts {
        let base_indices = ast.child_indices(declaration_index, "base");
        if base_indices.len() != 1 {
            return Err(format!(
                "self target newtype `{declaration_name}` has {} base type nodes",
                base_indices.len()
            ));
        }
        Some(target_type_fact(
            module,
            type_facts,
            base_indices[0],
            &format!("self target newtype `{declaration_name}` base"),
        )?)
    } else {
        None
    };
    Ok(SelfTargetNominalFact {
        name: declaration_name.to_string(),
        identity: format!("{}::{declaration_name}", module.identity),
        kind: kind.to_string(),
        type_parameters,
        members,
        base_type,
    })
}

pub(crate) struct DeclaredFunctionShape {
    type_parameters: usize,
    parameter_names: Vec<String>,
    parameter_defaults: Vec<bool>,
}

pub(crate) fn project_target_nominal_facts_with_ast_indexes(
    typed: &TypedPreviewResult,
    operations: &[Stage1LoweredOperation],
    ast_indexes: &[TargetAstIndex<'_>],
) -> Result<Vec<SelfTargetNominalFact>, String> {
    let mut nominals = Vec::new();
    let mut type_facts: TargetTypeFactIndex<'_> = std::collections::HashMap::new();
    for node in typed
        .nodes
        .iter()
        .filter(|node| node.kind == topaz_hir::TypedNodeKind::Type)
    {
        if node.ambient || node.ty.has_hole() {
            return Err(format!(
                "self target type fact {}:{}:{} is incomplete",
                node.module, node.span.lo, node.span.hi
            ));
        }
        if type_facts
            .entry(node.module.as_str())
            .or_default()
            .insert((node.span.lo, node.span.hi), &node.ty)
            .is_some()
        {
            return Err(format!(
                "self target type fact {}:{}:{} is duplicated",
                node.module, node.span.lo, node.span.hi
            ));
        }
    }
    let has_type_facts = !type_facts.is_empty();
    for declaration in typed.resolved.declarations.iter().filter(|declaration| {
        declaration.namespace == "type" && declaration.declaration_kind == "nominal-type"
    }) {
        let ast = ast_indexes.get(declaration.module_index).ok_or_else(|| {
            format!(
                "self target nominal `{}` refers to missing module {}",
                declaration.name, declaration.module_index
            )
        })?;
        nominals.push(target_nominal_fact(
            ast,
            &type_facts,
            has_type_facts,
            operations,
            &declaration.name,
            declaration.lo,
            declaration.hi,
        )?);
    }
    nominals.sort_by(|left, right| left.identity.cmp(&right.identity));
    nominals.dedup_by(|left, right| left.identity == right.identity);
    Ok(nominals)
}

#[cfg(test)]
pub(crate) fn project_target_nominal_facts(
    typed: &TypedPreviewResult,
    operations: &[Stage1LoweredOperation],
) -> Result<Vec<SelfTargetNominalFact>, String> {
    let ast_indexes = typed
        .resolved
        .modules
        .iter()
        .map(TargetAstIndex::new)
        .collect::<Vec<_>>();
    project_target_nominal_facts_with_ast_indexes(typed, operations, &ast_indexes)
}

pub(crate) fn declared_function_shape(
    ast: &TargetAstIndex<'_>,
    name_lo: u32,
    name_hi: u32,
) -> Result<Option<DeclaredFunctionShape>, String> {
    let module = ast.module;
    let Some(&name_index) = ast.name_indices(name_lo, name_hi).first() else {
        return Ok(None);
    };
    let Some(declaration_index) = module.ast[name_index].parent.map(|value| value as usize) else {
        return Ok(None);
    };
    let Some(declaration) = module.ast.get(declaration_index) else {
        return Err("self target export AST parent is outside the module AST".to_string());
    };
    if declaration.kind != "function-declaration" {
        return Ok(None);
    }
    let type_parameters = ast.child_indices(declaration_index, "typeParameters").len();
    let parameters = ast.child_indices(declaration_index, "parameters");
    let mut names = Vec::with_capacity(parameters.len());
    let mut defaults = Vec::with_capacity(parameters.len());
    for &parameter in parameters {
        let name_children = ast.child_indices(parameter, "name");
        if name_children.len() != 1 {
            return Err("self target function parameter name is missing or ambiguous".to_string());
        }
        let name = &module.ast[name_children[0]];
        let spelling = name
            .attributes
            .iter()
            .find_map(|attribute| (attribute.name == "spelling").then_some(&attribute.value))
            .and_then(|value| match value {
                topaz_kernel::CanonicalPreviewAstValue::String(value) => Some(value.clone()),
                _ => None,
            })
            .unwrap_or_else(|| module.source[name.lo as usize..name.hi as usize].to_string());
        if spelling.is_empty() {
            return Err("self target function parameter has an empty name".to_string());
        }
        names.push(spelling);
        defaults.push(!ast.child_indices(parameter, "default").is_empty());
    }
    Ok(Some(DeclaredFunctionShape {
        type_parameters,
        parameter_names: names,
        parameter_defaults: defaults,
    }))
}

pub(crate) fn project_target_adapter_facts_from_parts(
    typed: &TypedPreviewResult,
    lowered: &Stage1LoweringPreviewResult,
    producer: &'static str,
    source_set_id: &str,
    result_id: &str,
) -> Result<SelfTargetAdapterFacts, String> {
    let ast_indexes = typed
        .resolved
        .modules
        .iter()
        .map(TargetAstIndex::new)
        .collect::<Vec<_>>();
    let entry_modules = typed
        .resolved
        .modules
        .iter()
        .enumerate()
        .filter(|(_, module)| module.entry)
        .collect::<Vec<_>>();
    if entry_modules.len() != 1 {
        return Err(format!(
            "self target adapter requires exactly one entry module, found {}",
            entry_modules.len()
        ));
    }
    let (entry_index, entry) = entry_modules[0];
    let entry_ast = ast_indexes
        .get(entry_index)
        .ok_or_else(|| "self target entry module has no AST index".to_string())?;
    let mut exports = Vec::new();
    let nominals =
        project_target_nominal_facts_with_ast_indexes(typed, &lowered.operations, &ast_indexes)?;
    for export in typed
        .resolved
        .exports
        .iter()
        .filter(|export| export.module_index == entry_index)
    {
        typed
            .resolved
            .declarations
            .iter()
            .find(|declaration| {
                declaration.module_index == export.module_index
                    && declaration.namespace == export.namespace
                    && declaration.name == export.name
                    && declaration.lo == export.declaration_lo
                    && declaration.hi == export.declaration_hi
            })
            .ok_or_else(|| {
                format!(
                    "self target export `{}` has no exact resolved declaration",
                    export.name
                )
            })?;
        if export.namespace == "type" {
            continue;
        }
        if export.namespace != "value" {
            continue;
        }
        let typed = typed.exported_value_node(export).ok_or_else(|| {
            format!(
                "self target export `{}` has no exact typed value fact",
                export.name
            )
        })?;
        if typed.ambient || typed.ty.has_hole() {
            return Err(format!(
                "self target export `{}` contains an ambient or incomplete type",
                export.name
            ));
        }
        let declared =
            declared_function_shape(entry_ast, export.declaration_lo, export.declaration_hi)?;
        let (type_parameters, parameter_names, parameter_defaults, names_known) = match declared {
            Some(DeclaredFunctionShape {
                type_parameters,
                parameter_names,
                parameter_defaults,
            }) => (type_parameters, parameter_names, parameter_defaults, true),
            None => (0, Vec::new(), Vec::new(), false),
        };
        if let topaz_hir::SemanticType::Function { parameters, .. } = &typed.ty {
            if names_known && parameters.len() != parameter_names.len() {
                return Err(format!(
                    "self target export `{}` parameter facts disagree with its checked type",
                    export.name
                ));
            }
        } else if names_known {
            return Err(format!(
                "self target export `{}` is declared as a function but has a non-function type",
                export.name
            ));
        }
        exports.push(SelfTargetExportFact {
            name: export.name.clone(),
            ty: typed.ty.clone(),
            type_parameters,
            parameter_names,
            parameter_defaults,
            names_known,
        });
    }
    exports.sort_by(|left, right| left.name.cmp(&right.name));
    let mut operation_nominals = Vec::new();
    for operation in lowered.operations.iter().filter(|operation| {
        matches!(
            operation.semantic_type.as_str(),
            "enum" | "nominal-record" | "newtype"
        )
    }) {
        let mut candidates = typed
            .nodes
            .iter()
            .filter(|node| {
                node.module == operation.module
                    && node.span.lo == operation.lo
                    && node.span.hi == operation.hi
            })
            .filter_map(|node| match &node.ty {
                topaz_hir::SemanticType::Enum { identity, .. } => {
                    Some((identity.clone(), "enum".to_string()))
                }
                topaz_hir::SemanticType::NominalRecord { identity, .. } => {
                    Some((identity.clone(), "record".to_string()))
                }
                topaz_hir::SemanticType::Newtype { identity, .. } => {
                    Some((identity.clone(), "newtype".to_string()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();
        if candidates.len() != 1 {
            return Err(format!(
                "self target operation `{}` has {} exact nominal type facts",
                operation.id,
                candidates.len()
            ));
        }
        let (candidate_identity, kind) = candidates.remove(0);
        let identity = if nominals
            .iter()
            .any(|nominal| nominal.identity == candidate_identity && nominal.kind == kind)
        {
            candidate_identity
        } else {
            let matching = nominals
                .iter()
                .filter(|nominal| nominal.name == candidate_identity && nominal.kind == kind)
                .map(|nominal| nominal.identity.clone())
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return Err(format!(
                    "self target operation `{}` nominal `{candidate_identity}` has {} declaration matches",
                    operation.id,
                    matching.len()
                ));
            }
            matching[0].clone()
        };
        operation_nominals.push(SelfTargetOperationNominalFact {
            operation_id: operation.id.clone(),
            identity,
            kind,
        });
    }
    operation_nominals.sort_by(|left, right| left.operation_id.cmp(&right.operation_id));
    let mut runtime_requirements = lowered
        .operations
        .iter()
        .filter_map(|operation| {
            (!operation.runtime_leaf.is_empty()).then_some(operation.runtime_leaf.clone())
        })
        .collect::<Vec<_>>();
    runtime_requirements.sort();
    runtime_requirements.dedup();
    Ok(SelfTargetAdapterFacts {
        schema: SELF_TARGET_ADAPTER_FACTS_SCHEMA,
        producer,
        source_set_id: source_set_id.to_string(),
        result_id: result_id.to_string(),
        entry_module: entry.identity.clone(),
        exports,
        nominals,
        operation_nominals,
        runtime_requirements,
    })
}

/// Projects target-neutral export and nominal facts from a completed C2 product.
pub fn project_target_adapter_facts(
    product: &SelfCompilationProduct,
) -> Result<SelfTargetAdapterFacts, String> {
    require_completed_self_compilation_product(
        product,
        "self target adapter requires one completed C2 product",
    )?;
    project_target_adapter_facts_from_completed(product)
}

pub(crate) fn project_target_adapter_facts_from_completed(
    product: &SelfCompilationProduct,
) -> Result<SelfTargetAdapterFacts, String> {
    project_target_adapter_facts_from_parts(
        &product.typed,
        &product.lowered,
        product.compiler.producer,
        &product.target_source_set_id,
        &product.result_id,
    )
}

/// Project the nominal facts required to execute the compiler program image.
/// Both the direct Stage 0 and linked-image regeneration routes derive this
/// sidecar from the same admitted typed and lowered compiler response.
pub fn encode_compiler_program_target_facts(
    generated: &Stage1GeneratedPreviewResult,
) -> Result<String, String> {
    let typed = decode_stage1_typed_from_generated(generated)?;
    let lowered = decode_stage1_lowering_from_generated(generated)?;
    validate_self_compilation_outcome(generated, &typed, &lowered)?;
    let mut facts = project_target_adapter_facts_from_parts(
        &typed,
        &lowered,
        CompilerProducer::Stage1.identity(),
        &generated.provenance_source_set_id,
        "",
    )?;
    facts.result_id = stage1_sha256(encode_target_adapter_facts(&facts).as_bytes());
    Ok(encode_target_adapter_facts(&facts))
}

/// Embeds canonical target facts into generated compiler Rust exactly once.
pub fn seal_compiler_program_target_facts(
    generated: &mut Stage1GeneratedPreviewResult,
) -> Result<(), String> {
    let compiler_target_facts = encode_compiler_program_target_facts(generated)?;
    if compiler_target_facts.contains("\"##") {
        return Err(
            "compiler target facts collide with the generated Rust raw-string delimiter"
                .to_string(),
        );
    }
    if !generated.generated_rust.ends_with('\n') {
        generated.generated_rust.push('\n');
    }
    generated
        .generated_rust
        .push_str("pub const TOPAZ_COMPILER_TARGET_FACTS_JSON: &str = r##\"");
    generated.generated_rust.push_str(&compiler_target_facts);
    generated.generated_rust.push_str("\"##;\n");
    Ok(())
}

/// Encode the mechanically projected target facts in the private runtime
/// schema consumed by every self target host.
pub fn encode_target_adapter_facts(facts: &SelfTargetAdapterFacts) -> String {
    let json_string = |value: &str| {
        let mut result = String::with_capacity(value.len() + 2);
        result.push('"');
        for character in value.chars() {
            match character {
                '"' => result.push_str("\\\""),
                '\\' => result.push_str("\\\\"),
                '\u{08}' => result.push_str("\\b"),
                '\u{0c}' => result.push_str("\\f"),
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\t' => result.push_str("\\t"),
                character if character <= '\u{1f}' => {
                    use std::fmt::Write as _;
                    let _ = write!(result, "\\u{:04x}", character as u32);
                }
                character => result.push(character),
            }
        }
        result.push('"');
        result
    };
    let runtime_requirements = facts
        .runtime_requirements
        .iter()
        .map(|requirement| json_string(requirement))
        .collect::<Vec<_>>()
        .join(",");
    let nominals = facts
        .nominals
        .iter()
        .map(|nominal| {
            let members = nominal
                .members
                .iter()
                .map(|member| {
                    let default_operation_id = member
                        .default_operation_id
                        .as_deref()
                        .map(&json_string)
                        .unwrap_or_else(|| "null".to_string());
                    let types = member
                        .types
                        .iter()
                        .map(|ty| {
                            json_string(&stage1_encode_json(
                                &topaz_kernel::semantic_type_atoms_json(ty),
                            ))
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    format!(
                        "{{\"name\":{},\"arity\":{},\"defaultOperationId\":{},\"types\":[{}]}}",
                        json_string(&member.name),
                        member.arity,
                        default_operation_id,
                        types,
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let type_parameters = nominal
                .type_parameters
                .iter()
                .map(|parameter| json_string(parameter))
                .collect::<Vec<_>>()
                .join(",");
            let base_type = nominal
                .base_type
                .as_ref()
                .map(|ty| {
                    json_string(&stage1_encode_json(
                        &topaz_kernel::semantic_type_atoms_json(ty),
                    ))
                })
                .unwrap_or_else(|| "null".to_string());
            format!(
                "{{\"name\":{},\"identity\":{},\"kind\":{},\"typeParameters\":[{}],\"members\":[{}],\"baseType\":{}}}",
                json_string(&nominal.name),
                json_string(&nominal.identity),
                json_string(&nominal.kind),
                type_parameters,
                members,
                base_type,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let operation_nominals = facts
        .operation_nominals
        .iter()
        .map(|operation| {
            format!(
                "{{\"operationId\":{},\"identity\":{},\"kind\":{}}}",
                json_string(&operation.operation_id),
                json_string(&operation.identity),
                json_string(&operation.kind),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{\"schema\":{},\"producer\":{},\"sourceSetId\":{},",
            "\"resultId\":{},\"entryModule\":{},\"runtimeRequirements\":[{}],",
            "\"nominals\":[{}],\"operationNominals\":[{}]}}"
        ),
        json_string(facts.schema),
        json_string(facts.producer),
        json_string(&facts.source_set_id),
        json_string(&facts.result_id),
        json_string(&facts.entry_module),
        runtime_requirements,
        nominals,
        operation_nominals,
    )
}

/// Recover a shared runtime diagnostic preserved by the self target adapter.
pub fn decode_self_product_runtime_diagnostic(error: &str) -> Option<topaz_value::RtError> {
    topaz_stage1_runtime::decode_runtime_diagnostic(error)
}

/// Admits generated Rust size, template markers, and required compiler entry points.
pub fn validate_stage1_generated_rust(
    generated: &str,
    max_generated_rust_bytes: u64,
) -> Result<(), String> {
    if generated.is_empty() {
        return Err("Stage 1 generated Rust is empty".to_string());
    }
    if generated.len() as u64 > max_generated_rust_bytes {
        return Err(format!(
            "Stage 1 generated Rust uses {} bytes, limit {max_generated_rust_bytes}",
            generated.len()
        ));
    }
    let fixed_point_capable = generated.contains("TOPAZ_COMPILER_IR_JSON");
    let required = if fixed_point_capable {
        &[
            "TOPAZ_COMPILER_RUNTIME_REGISTRY_SCHEMA",
            "TOPAZ_COMPILER_RUNTIME_TEMPLATE",
            "TOPAZ_COMPILER_IR_SCHEMA",
            "TOPAZ_COMPILER_IR_PAYLOAD_SCHEMA",
            "TOPAZ_COMPILER_SOURCE_SET",
            "TOPAZ_COMPILER_IR_JSON",
            "TOPAZ_COMPILER_RUNTIME_LEAVES",
            "compiler_preview_i64",
        ][..]
    } else {
        &[
            "TOPAZ_STAGE1_RUNTIME_REGISTRY_SCHEMA",
            "TOPAZ_STAGE1_RUNTIME_TEMPLATE",
            "TOPAZ_STAGE1_IR_SCHEMA",
            "TOPAZ_STAGE1_IR_PAYLOAD_SCHEMA",
            "TOPAZ_STAGE1_SOURCE_SET",
            "TOPAZ_STAGE1_IR_JSON",
            "TOPAZ_STAGE1_RUNTIME_LEAVES",
            "stage1_preview_i64",
        ][..]
    };
    for required in required {
        if !generated.contains(required) {
            return Err(format!(
                "Stage 1 generated Rust omits required boundary `{required}`"
            ));
        }
    }
    let template_lines = generated
        .lines()
        .filter(|line| {
            !line.starts_with("pub const TOPAZ_STAGE1_IR_JSON:")
                && !line.starts_with("pub const TOPAZ_COMPILER_IR_JSON:")
        })
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "topaz_parser",
        "topaz_resolve",
        "topaz_check",
        "topaz_lower",
        "topaz_emit",
        "topaz_interp",
    ] {
        if template_lines.contains(forbidden) {
            return Err(format!(
                "Stage 1 generated Rust reaches forbidden target dependency `{forbidden}`"
            ));
        }
    }
    Ok(())
}
