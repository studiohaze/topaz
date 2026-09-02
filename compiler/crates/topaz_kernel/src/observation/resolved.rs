use super::bundle::*;
use super::validate::schema::validate_schema_registry;
use super::*;

fn resolved_namespace(namespace: topaz_resolve::ResolvedNamespace) -> &'static str {
    match namespace {
        topaz_resolve::ResolvedNamespace::Value => "value",
        topaz_resolve::ResolvedNamespace::Type => "type",
        topaz_resolve::ResolvedNamespace::Module => "module",
    }
}

fn resolved_scope_kind(kind: topaz_resolve::ResolvedScopeKind) -> &'static str {
    match kind {
        topaz_resolve::ResolvedScopeKind::Module => "module",
        topaz_resolve::ResolvedScopeKind::Function => "function",
        topaz_resolve::ResolvedScopeKind::Block => "block",
        topaz_resolve::ResolvedScopeKind::Pattern => "pattern",
        topaz_resolve::ResolvedScopeKind::Lambda => "lambda",
        topaz_resolve::ResolvedScopeKind::Comprehension => "comprehension",
        topaz_resolve::ResolvedScopeKind::Using => "using",
    }
}

fn resolved_declaration_kind(kind: topaz_resolve::ResolvedDeclarationKind) -> &'static str {
    match kind {
        topaz_resolve::ResolvedDeclarationKind::NamespaceImport => "namespace-import",
        topaz_resolve::ResolvedDeclarationKind::SelectedImport => "selected-import",
        topaz_resolve::ResolvedDeclarationKind::Function => "function",
        topaz_resolve::ResolvedDeclarationKind::TypeAlias => "type-alias",
        topaz_resolve::ResolvedDeclarationKind::NominalType => "nominal-type",
        topaz_resolve::ResolvedDeclarationKind::Protocol => "protocol",
        topaz_resolve::ResolvedDeclarationKind::Let => "let",
        topaz_resolve::ResolvedDeclarationKind::Const => "const",
        topaz_resolve::ResolvedDeclarationKind::Parameter => "parameter",
        topaz_resolve::ResolvedDeclarationKind::Pattern => "pattern",
        topaz_resolve::ResolvedDeclarationKind::Using => "using",
    }
}

fn resolved_reference_role(role: topaz_resolve::ResolvedReferenceRole) -> &'static str {
    match role {
        topaz_resolve::ResolvedReferenceRole::Read => "read",
        topaz_resolve::ResolvedReferenceRole::Write => "write",
        topaz_resolve::ResolvedReferenceRole::NamespaceMember => "namespace-member",
        topaz_resolve::ResolvedReferenceRole::Type => "type",
    }
}

fn scope_id(source_id: &str, ordinal: u32) -> String {
    format!("scope:{source_id}:{ordinal:08x}")
}

fn node_for_span(
    identity_nodes: &BTreeMap<(u32, u32, u32), String>,
    file: topaz_diag::FileId,
    value: Span,
) -> Option<&str> {
    identity_nodes
        .get(&(file.0, value.lo, value.hi))
        .map(String::as_str)
}

fn symbol_for_declaration(
    identity_nodes: &BTreeMap<(u32, u32, u32), String>,
    file: topaz_diag::FileId,
    value: Span,
    namespace: topaz_resolve::ResolvedNamespace,
) -> Option<String> {
    node_for_span(identity_nodes, file, value)
        .map(|node| format!("sym:{node}:{}:0", resolved_namespace(namespace)))
}

pub(super) fn resolved_rows(
    unit: &KernelUnit,
    sources: &BTreeMap<u32, SourceIdentity>,
    identity_nodes: &BTreeMap<(u32, u32, u32), String>,
) -> Vec<JsonValue> {
    let mut rows = Vec::new();
    for (ordinal, module) in unit.resolved.modules.iter().enumerate() {
        let source = &sources[&module.file.0];
        rows.push(object([
            ("entry", boolean(module.is_entry)),
            ("extern", boolean(module.is_extern)),
            ("generatedStd", boolean(module.is_generated_std)),
            (
                "externIdentity",
                if module.is_extern {
                    string(format!("extern:{}", module.identity))
                } else {
                    JsonValue::Null
                },
            ),
            ("identity", string(&module.identity)),
            ("initializationOrdinal", unsigned(ordinal as u64)),
            ("path", string(&module.path)),
            ("rowKind", string("module")),
            ("schema", string(RESOLVED_SCHEMA)),
            ("sourceId", string(&source.source_id)),
        ]));
    }
    for (ordinal, (from, to)) in unit.resolved.import_edges.iter().enumerate() {
        rows.push(object([
            ("from", string(from)),
            ("ordinal", unsigned(ordinal as u64)),
            ("rowKind", string("import-edge")),
            ("schema", string(RESOLVED_SCHEMA)),
            ("to", string(to)),
        ]));
    }
    for scope in &unit.resolved.name_facts.scopes {
        let source = &sources[&scope.file.0];
        rows.push(object([
            ("kind", string(resolved_scope_kind(scope.kind))),
            (
                "ownerNodeId",
                node_for_span(identity_nodes, scope.file, scope.owner)
                    .map_or(JsonValue::Null, string),
            ),
            (
                "parentScopeId",
                scope
                    .parent_ordinal
                    .map(|ordinal| string(scope_id(&source.source_id, ordinal)))
                    .unwrap_or(JsonValue::Null),
            ),
            ("rowKind", string("scope")),
            ("schema", string(RESOLVED_SCHEMA)),
            (
                "scopeId",
                string(scope_id(&source.source_id, scope.ordinal)),
            ),
            ("sourceId", string(&source.source_id)),
            ("span", span(&source.source_id, scope.owner)),
        ]));
    }
    for declaration in &unit.resolved.name_facts.declarations {
        let source = &sources[&declaration.file.0];
        let namespace = resolved_namespace(declaration.namespace);
        let symbol_id = symbol_for_declaration(
            identity_nodes,
            declaration.file,
            declaration.span,
            declaration.namespace,
        );
        rows.push(object([
            (
                "declarationKind",
                string(resolved_declaration_kind(declaration.kind)),
            ),
            (
                "declarationNodeId",
                node_for_span(identity_nodes, declaration.file, declaration.span)
                    .map_or(JsonValue::Null, string),
            ),
            ("exported", boolean(declaration.exported)),
            ("name", string(&declaration.name)),
            ("namespace", string(namespace)),
            (
                "nominalIdentity",
                if matches!(
                    declaration.kind,
                    topaz_resolve::ResolvedDeclarationKind::NominalType
                ) {
                    symbol_id
                        .as_ref()
                        .map(|symbol| string(format!("nominal:{symbol}")))
                        .unwrap_or(JsonValue::Null)
                } else {
                    JsonValue::Null
                },
            ),
            ("rowKind", string("declaration")),
            ("schema", string(RESOLVED_SCHEMA)),
            (
                "scopeId",
                string(scope_id(&source.source_id, declaration.scope_ordinal)),
            ),
            ("sourceId", string(&source.source_id)),
            ("span", span(&source.source_id, declaration.span)),
            (
                "symbolId",
                symbol_id.as_ref().map_or(JsonValue::Null, string),
            ),
            (
                "targetModule",
                declaration
                    .target_module
                    .as_ref()
                    .map_or(JsonValue::Null, string),
            ),
            (
                "targetName",
                declaration
                    .target_name
                    .as_ref()
                    .map_or(JsonValue::Null, string),
            ),
        ]));
    }
    for reference in &unit.resolved.name_facts.references {
        let source = &sources[&reference.file.0];
        let target_symbol_id =
            reference
                .target_file
                .zip(reference.target_span)
                .and_then(|(file, target_span)| {
                    symbol_for_declaration(
                        identity_nodes,
                        file,
                        target_span,
                        reference.target_namespace.unwrap_or(reference.namespace),
                    )
                });
        rows.push(object([
            ("name", string(&reference.name)),
            ("namespace", string(resolved_namespace(reference.namespace))),
            (
                "referenceNodeId",
                node_for_span(identity_nodes, reference.file, reference.span)
                    .map_or(JsonValue::Null, string),
            ),
            ("role", string(resolved_reference_role(reference.role))),
            ("rowKind", string("reference")),
            ("schema", string(RESOLVED_SCHEMA)),
            (
                "scopeId",
                string(scope_id(&source.source_id, reference.scope_ordinal)),
            ),
            ("sourceId", string(&source.source_id)),
            ("span", span(&source.source_id, reference.span)),
            (
                "targetModule",
                reference
                    .target_module
                    .as_ref()
                    .map_or(JsonValue::Null, string),
            ),
            (
                "targetNamespace",
                reference
                    .target_namespace
                    .map(|namespace| string(resolved_namespace(namespace)))
                    .unwrap_or(JsonValue::Null),
            ),
            (
                "targetName",
                reference
                    .target_name
                    .as_ref()
                    .map_or(JsonValue::Null, string),
            ),
            (
                "targetSymbolId",
                target_symbol_id.as_ref().map_or(JsonValue::Null, string),
            ),
        ]));
    }
    for export in &unit.resolved.name_facts.exports {
        let source = &sources[&export.file.0];
        let symbol_id = symbol_for_declaration(
            identity_nodes,
            export.file,
            export.declaration_span,
            export.namespace,
        );
        rows.push(object([
            ("name", string(&export.name)),
            ("namespace", string(resolved_namespace(export.namespace))),
            ("rowKind", string("export")),
            ("schema", string(RESOLVED_SCHEMA)),
            ("sourceId", string(&source.source_id)),
            (
                "symbolId",
                symbol_id.as_ref().map_or(JsonValue::Null, string),
            ),
        ]));
    }
    rows
}

/// Builds a resolved-layer bundle from the self front end's canonical rows.
pub fn build_resolved_preview_observation(
    input: ResolvedPreviewObservationInput<'_>,
) -> Result<ObservationBundle, String> {
    if input.request.terminal_phase() != crate::TerminalPhase::Resolved {
        return Err("resolved preview requires the resolved terminal phase".to_string());
    }
    let ResolvedPreviewFiles {
        mut files,
        request_digest,
        ..
    } = build_resolved_preview_files(input)?;
    let response = encode(&object([
        ("highestCompletedPhase", string("resolved")),
        (
            "phases",
            object([
                ("ast", string("produced")),
                ("lowered", string("not-requested")),
                ("resolved", string("produced")),
                ("rustSource", string("not-requested")),
                ("tokens", string("produced")),
                ("typed", string("not-requested")),
            ]),
        ),
        (
            "projectionDigests",
            object([
                ("ast", string(sha256(&files["ast.jsonl"].1))),
                ("diagnostics", string(sha256(&files["diagnostics.jsonl"].1))),
                ("lowered", JsonValue::Null),
                ("resolved", string(sha256(&files["resolved.jsonl"].1))),
                ("rustSource", JsonValue::Null),
                ("sourceSet", string(sha256(&files["source-set.jsonl"].1))),
                ("tokens", string(sha256(&files["tokens.jsonl"].1))),
                ("typed", JsonValue::Null),
            ]),
        ),
        ("requestDigest", string(request_digest)),
        ("schema", string(crate::RESPONSE_SCHEMA)),
        (
            "status",
            string(if input.diagnostics.is_empty() {
                "completed"
            } else {
                "rejected"
            }),
        ),
    ]));
    files.insert(
        "response.json".to_string(),
        (crate::RESPONSE_SCHEMA.to_string(), response),
    );
    finish_observation(files, input.request.budgets().max_projection_bytes, true)
}

pub(super) type PreviewNodeOrdinalMap = HashMap<(u32, u32), usize>;

pub(super) struct ResolvedPreviewFiles {
    pub(super) files: BTreeMap<String, (String, Vec<u8>)>,
    pub(super) request_digest: String,
    pub(super) node_ordinals: Vec<PreviewNodeOrdinalMap>,
}

fn preview_node_ordinals(modules: &[CanonicalPreviewModule]) -> Vec<PreviewNodeOrdinalMap> {
    modules
        .iter()
        .map(|module| {
            let mut ordinals = HashMap::with_capacity(module.ast.len());
            for (ordinal, node) in module.ast.iter().enumerate() {
                ordinals.entry((node.lo, node.hi)).or_insert(ordinal);
            }
            ordinals
        })
        .collect()
}

pub(super) fn build_resolved_preview_files(
    input: ResolvedPreviewObservationInput<'_>,
) -> Result<ResolvedPreviewFiles, String> {
    let ResolvedPreviewObservationInput {
        request,
        modules,
        edges,
        scopes,
        declarations,
        references,
        exports,
        diagnostics,
    } = input;
    validate_schema_registry()?;
    let node_ordinals = preview_node_ordinals(modules);
    let budgets = request.budgets();
    let total_source_bytes = modules
        .iter()
        .map(|module| module.source.len() as u64)
        .sum::<u64>();
    let raw_tokens = modules
        .iter()
        .map(|module| module.raw.len() as u64)
        .sum::<u64>();
    let layout_tokens = modules
        .iter()
        .map(|module| module.layout.len() as u64)
        .sum::<u64>();
    let ast_nodes = modules
        .iter()
        .map(|module| module.ast.len() as u64)
        .sum::<u64>();
    for (label, observed, limit) in [
        (
            "source-fact",
            request
                .facts()
                .values()
                .filter(|fact| matches!(fact, crate::HostFact::Source(_)))
                .count() as u64,
            budgets.max_source_facts,
        ),
        (
            "source-byte",
            total_source_bytes,
            budgets.max_total_source_bytes,
        ),
        ("raw-token", raw_tokens, budgets.max_raw_tokens),
        ("layout-token", layout_tokens, budgets.max_layout_tokens),
        ("ast-node", ast_nodes, budgets.max_ast_nodes),
        (
            "diagnostic",
            diagnostics.len() as u64,
            budgets.max_diagnostics,
        ),
    ] {
        if observed > limit {
            return Err(format!(
                "{label} resource limit: observed {observed}, limit {limit}"
            ));
        }
    }

    let mut source_order = (0..modules.len()).collect::<Vec<_>>();
    source_order.sort_by_key(|index| (&modules[*index].identity, &modules[*index].path));
    let mut identities = BTreeMap::<usize, SourceIdentity>::new();
    for (ordinal, index) in source_order.iter().copied().enumerate() {
        let module = &modules[index];
        identities.insert(
            index,
            SourceIdentity {
                source_id: source_id(&module.identity, &module.path),
                module: module.identity.clone(),
                path: topaz_resolve::normalize_path(&module.path),
                ordinal: ordinal as u64,
            },
        );
    }

    let mut files = BTreeMap::<String, (String, Vec<u8>)>::new();
    let mut source_members = BTreeMap::<String, (String, String, u64)>::new();
    let mut source_rows = Vec::new();
    let mut token_bytes = Vec::new();
    let mut ast_bytes = Vec::new();
    for index in source_order {
        let module = &modules[index];
        let identity = &identities[&index];
        let member = format!("sources/{:06}.tpz", identity.ordinal);
        let digest = sha256(module.source.as_bytes());
        files.insert(
            member.clone(),
            (
                "topaz.source/utf8".to_string(),
                module.source.as_bytes().to_vec(),
            ),
        );
        source_members.insert(
            identity.path.clone(),
            (member.clone(), digest.clone(), module.source.len() as u64),
        );
        source_rows.push(object([
            ("byteLength", unsigned(module.source.len() as u64)),
            ("contentSha256", string(&digest)),
            ("entry", boolean(module.entry)),
            ("member", string(&member)),
            ("module", string(&identity.module)),
            (
                "originRole",
                string(if module.generated_std {
                    "generated-std"
                } else if module.extern_module {
                    "extern"
                } else {
                    "source"
                }),
            ),
            ("path", string(&identity.path)),
            ("rowKind", string("source")),
            ("schema", string(SOURCE_SET_SCHEMA)),
            ("sourceId", string(&identity.source_id)),
            ("sourceOrdinal", unsigned(identity.ordinal)),
        ]));

        for row in preview_token_rows(identity, &module.source, &module.raw, &module.layout)? {
            token_bytes.extend_from_slice(&encode(&row));
        }
        for row in preview_ast_rows(identity, &module.source, &module.ast)? {
            ast_bytes.extend_from_slice(&encode(&row));
        }
    }
    for (query, fact) in request.facts() {
        if matches!(
            query,
            crate::HostQuery::ReadSource { logical_path, .. }
                if logical_path.starts_with("std/") && source_members.contains_key(logical_path)
        ) {
            // Rust Stage 0 resolves the embedded standard library without a
            // host query. The Topaz preview asks the host for the same
            // embedded bytes while bootstrapping, but that implementation
            // detail is not part of the canonical source-set observation.
            continue;
        }
        let JsonValue::Object(fields) = source_fact_json(query, fact, &source_members) else {
            unreachable!("host fact projection is an object");
        };
        let mut fields = fields.as_ref().clone();
        fields.insert("rowKind".into(), string("host-fact"));
        fields.insert("schema".into(), string(SOURCE_SET_SCHEMA));
        source_rows.push(JsonValue::Object(fields.into()));
    }
    files.insert(
        "source-set.jsonl".to_string(),
        (SOURCE_SET_SCHEMA.to_string(), encode_jsonl(&source_rows)),
    );
    files.insert(
        "tokens.jsonl".to_string(),
        (TOKENS_SCHEMA.to_string(), token_bytes),
    );
    files.insert("ast.jsonl".to_string(), (AST_SCHEMA.to_string(), ast_bytes));

    let node_id = |module_index: usize, lo: u32, hi: u32| -> Option<String> {
        let ordinal = node_ordinals.get(module_index)?.get(&(lo, hi))?;
        Some(format!(
            "{}#n{ordinal:08x}",
            identities.get(&module_index)?.source_id
        ))
    };
    let symbol_id = |module_index: usize, lo: u32, hi: u32, namespace: &str| -> Option<String> {
        node_id(module_index, lo, hi).map(|node| format!("sym:{node}:{namespace}:0"))
    };

    let mut resolved_bytes = Vec::new();
    let mut push_resolved = |row: JsonValue| {
        resolved_bytes.extend_from_slice(&encode(&row));
    };
    for (ordinal, module) in modules.iter().enumerate() {
        let identity = identities
            .get(&ordinal)
            .ok_or_else(|| format!("resolved preview module {ordinal} lacks source identity"))?;
        push_resolved(object([
            ("entry", boolean(module.entry)),
            ("extern", boolean(module.extern_module)),
            ("generatedStd", boolean(module.generated_std)),
            (
                "externIdentity",
                if module.extern_module {
                    string(format!("extern:{}", module.identity))
                } else {
                    JsonValue::Null
                },
            ),
            ("identity", string(&module.identity)),
            ("initializationOrdinal", unsigned(ordinal as u64)),
            ("path", string(&identity.path)),
            ("rowKind", string("module")),
            ("schema", string(RESOLVED_SCHEMA)),
            ("sourceId", string(&identity.source_id)),
        ]));
    }
    for (ordinal, edge) in edges.iter().enumerate() {
        push_resolved(object([
            ("from", string(&edge.from)),
            ("ordinal", unsigned(ordinal as u64)),
            ("rowKind", string("import-edge")),
            ("schema", string(RESOLVED_SCHEMA)),
            ("to", string(&edge.to)),
        ]));
    }
    for scope in scopes {
        let identity = identities.get(&scope.module_index).ok_or_else(|| {
            format!(
                "resolved preview scope references module {}",
                scope.module_index
            )
        })?;
        if scope.lo > scope.hi || scope.hi as usize > modules[scope.module_index].source.len() {
            return Err("resolved preview scope span is outside its source".to_string());
        }
        push_resolved(object([
            ("kind", string(&scope.kind)),
            (
                "ownerNodeId",
                node_id(scope.module_index, scope.lo, scope.hi).map_or(JsonValue::Null, string),
            ),
            (
                "parentScopeId",
                scope.parent_ordinal.map_or(JsonValue::Null, |parent| {
                    string(scope_id(&identity.source_id, parent))
                }),
            ),
            ("rowKind", string("scope")),
            ("schema", string(RESOLVED_SCHEMA)),
            (
                "scopeId",
                string(scope_id(&identity.source_id, scope.ordinal)),
            ),
            ("sourceId", string(&identity.source_id)),
            (
                "span",
                object([
                    ("hi", unsigned(scope.hi.into())),
                    ("lo", unsigned(scope.lo.into())),
                    ("sourceId", string(&identity.source_id)),
                ]),
            ),
        ]));
    }
    for declaration in declarations {
        let identity = identities.get(&declaration.module_index).ok_or_else(|| {
            format!(
                "resolved preview declaration references module {}",
                declaration.module_index
            )
        })?;
        let symbol = symbol_id(
            declaration.module_index,
            declaration.lo,
            declaration.hi,
            &declaration.namespace,
        );
        push_resolved(object([
            ("declarationKind", string(&declaration.declaration_kind)),
            (
                "declarationNodeId",
                node_id(declaration.module_index, declaration.lo, declaration.hi)
                    .map_or(JsonValue::Null, string),
            ),
            ("exported", boolean(declaration.exported)),
            ("name", string(&declaration.name)),
            ("namespace", string(&declaration.namespace)),
            (
                "nominalIdentity",
                if declaration.declaration_kind == "nominal-type" {
                    symbol
                        .as_ref()
                        .map(|value| string(format!("nominal:{value}")))
                        .unwrap_or(JsonValue::Null)
                } else {
                    JsonValue::Null
                },
            ),
            ("rowKind", string("declaration")),
            ("schema", string(RESOLVED_SCHEMA)),
            (
                "scopeId",
                string(scope_id(&identity.source_id, declaration.scope_ordinal)),
            ),
            ("sourceId", string(&identity.source_id)),
            (
                "span",
                object([
                    ("hi", unsigned(declaration.hi.into())),
                    ("lo", unsigned(declaration.lo.into())),
                    ("sourceId", string(&identity.source_id)),
                ]),
            ),
            ("symbolId", symbol.as_ref().map_or(JsonValue::Null, string)),
            (
                "targetModule",
                declaration
                    .target_module
                    .as_ref()
                    .map_or(JsonValue::Null, string),
            ),
            (
                "targetName",
                declaration
                    .target_name
                    .as_ref()
                    .map_or(JsonValue::Null, string),
            ),
        ]));
    }
    for reference in references {
        let identity = identities.get(&reference.module_index).ok_or_else(|| {
            format!(
                "resolved preview reference references module {}",
                reference.module_index
            )
        })?;
        let target_symbol = reference.target_module_index.and_then(|module_index| {
            symbol_id(
                module_index,
                reference.target_lo,
                reference.target_hi,
                reference
                    .target_namespace
                    .as_deref()
                    .unwrap_or(&reference.namespace),
            )
        });
        push_resolved(object([
            ("name", string(&reference.name)),
            ("namespace", string(&reference.namespace)),
            (
                "referenceNodeId",
                node_id(reference.module_index, reference.lo, reference.hi)
                    .map_or(JsonValue::Null, string),
            ),
            ("role", string(&reference.role)),
            ("rowKind", string("reference")),
            ("schema", string(RESOLVED_SCHEMA)),
            (
                "scopeId",
                string(scope_id(&identity.source_id, reference.scope_ordinal)),
            ),
            ("sourceId", string(&identity.source_id)),
            (
                "span",
                object([
                    ("hi", unsigned(reference.hi.into())),
                    ("lo", unsigned(reference.lo.into())),
                    ("sourceId", string(&identity.source_id)),
                ]),
            ),
            (
                "targetModule",
                reference
                    .target_module
                    .as_ref()
                    .map_or(JsonValue::Null, string),
            ),
            (
                "targetNamespace",
                reference
                    .target_namespace
                    .as_ref()
                    .map_or(JsonValue::Null, string),
            ),
            (
                "targetName",
                reference
                    .target_name
                    .as_ref()
                    .map_or(JsonValue::Null, string),
            ),
            (
                "targetSymbolId",
                target_symbol.as_ref().map_or(JsonValue::Null, string),
            ),
        ]));
    }
    for export in exports {
        let identity = identities.get(&export.module_index).ok_or_else(|| {
            format!(
                "resolved preview export references module {}",
                export.module_index
            )
        })?;
        let symbol = symbol_id(
            export.module_index,
            export.declaration_lo,
            export.declaration_hi,
            &export.namespace,
        );
        push_resolved(object([
            ("name", string(&export.name)),
            ("namespace", string(&export.namespace)),
            ("rowKind", string("export")),
            ("schema", string(RESOLVED_SCHEMA)),
            ("sourceId", string(&identity.source_id)),
            ("symbolId", symbol.as_ref().map_or(JsonValue::Null, string)),
        ]));
    }
    files.insert(
        "resolved.jsonl".to_string(),
        (RESOLVED_SCHEMA.to_string(), resolved_bytes),
    );

    let mut diagnostic_rows = Vec::new();
    for (ordinal, diagnostic) in diagnostics.iter().enumerate() {
        let identity = identities.get(&diagnostic.module_index).ok_or_else(|| {
            format!(
                "resolved preview diagnostic references module {}",
                diagnostic.module_index
            )
        })?;
        diagnostic_rows.push(object([
            ("code", string(&diagnostic.code)),
            ("message", string(&diagnostic.message)),
            ("notes", array(diagnostic.notes.iter().map(string))),
            ("ordinal", unsigned(ordinal as u64)),
            (
                "primary",
                object([
                    ("message", string("")),
                    (
                        "span",
                        object([
                            ("hi", unsigned(diagnostic.hi.into())),
                            ("lo", unsigned(diagnostic.lo.into())),
                            ("sourceId", string(&identity.source_id)),
                        ]),
                    ),
                ]),
            ),
            ("producerPhase", string("front-end")),
            ("profileRule", JsonValue::Null),
            ("schema", string(DIAGNOSTICS_SCHEMA)),
            ("secondary", array([])),
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

    let request_bytes = encode(&request_json(request, &source_members));
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
        "provenance.json".to_string(),
        (
            crate::PROVENANCE_SCHEMA.to_string(),
            encode(&provenance_json_with(
                &crate::BootstrapProvenance::topaz_front_end_preview(),
                Vec::new(),
            )),
        ),
    );
    Ok(ResolvedPreviewFiles {
        files,
        request_digest,
        node_ordinals,
    })
}
