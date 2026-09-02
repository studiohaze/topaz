use super::lowered::*;
use super::resolved::*;
use super::tokens_ast::*;
use super::typed::*;
use super::validate::schema::validate_schema_registry;
use super::*;

fn source_identities(unit: &KernelUnit) -> BTreeMap<u32, SourceIdentity> {
    let mut modules = unit.resolved.modules.iter().collect::<Vec<_>>();
    modules.sort_by_key(|module| (&module.identity, &module.path));
    let mut sources: BTreeMap<u32, SourceIdentity> = modules
        .into_iter()
        .enumerate()
        .map(|(ordinal, module)| {
            (
                module.file.0,
                SourceIdentity {
                    source_id: source_id(&module.identity, &module.path),
                    module: module.identity.clone(),
                    path: module.path.clone(),
                    ordinal: ordinal as u64,
                },
            )
        })
        .collect();
    let mut diagnostic_files = BTreeSet::new();
    for diagnostic in &unit.resolved.diagnostics {
        diagnostic_files.insert(diagnostic.primary.span.file.0);
        diagnostic_files.extend(diagnostic.secondary.iter().map(|label| label.span.file.0));
    }
    for file in diagnostic_files {
        if sources.contains_key(&file) {
            continue;
        }
        let source = unit.resolved.map.file(topaz_diag::FileId(file));
        let path = topaz_resolve::normalize_path(source.name());
        let ordinal = sources.len() as u64;
        sources.insert(
            file,
            SourceIdentity {
                source_id: source_id("diagnostic", &path),
                module: "diagnostic".to_string(),
                path,
                ordinal,
            },
        );
    }
    sources
}

fn diagnostic_rows(
    diagnostics: &[Diagnostic],
    sources: &BTreeMap<u32, SourceIdentity>,
) -> Vec<JsonValue> {
    diagnostics
        .iter()
        .enumerate()
        .map(|(ordinal, diagnostic)| {
            let source = &sources[&diagnostic.primary.span.file.0];
            object([
                ("code", string(diagnostic.code.as_str())),
                ("message", string(&diagnostic.message)),
                ("notes", array(diagnostic.notes.iter().map(string))),
                ("ordinal", unsigned(ordinal as u64)),
                (
                    "primary",
                    object([
                        ("message", string(&diagnostic.primary.message)),
                        ("span", span(&source.source_id, diagnostic.primary.span)),
                    ]),
                ),
                ("producerPhase", string("front-end")),
                ("profileRule", JsonValue::Null),
                ("schema", string(DIAGNOSTICS_SCHEMA)),
                (
                    "secondary",
                    array(diagnostic.secondary.iter().map(|label| {
                        let source = &sources[&label.span.file.0];
                        object([
                            ("message", string(&label.message)),
                            ("span", span(&source.source_id, label.span)),
                        ])
                    })),
                ),
                (
                    "severity",
                    string(match diagnostic.severity {
                        Severity::Error => "error",
                        Severity::Warning => "warning",
                    }),
                ),
            ])
        })
        .collect()
}

pub(super) fn source_fact_json(
    query: &crate::HostQuery,
    fact: &crate::HostFact,
    source_members: &BTreeMap<String, (String, String, u64)>,
) -> JsonValue {
    use crate::{
        ContainmentFact, DirectoryEntryKind, DirectoryFact, HostFact, HostQuery, SourceFact,
    };
    let kind = match query {
        HostQuery::ReadSource { .. } => "read-source",
        HostQuery::ListDirectory { .. } => "list-directory",
        HostQuery::PhysicalContainment { .. } => "physical-containment",
    };
    let value = match fact {
        HostFact::Source(SourceFact::Present(source)) => {
            let (member, digest, bytes) = source_members
                .get(query.logical_path())
                .cloned()
                .unwrap_or_else(|| {
                    (
                        String::new(),
                        sha256(source.as_bytes()),
                        source.len() as u64,
                    )
                });
            object([
                ("byteLength", unsigned(bytes)),
                ("contentSha256", string(digest)),
                ("member", string(member)),
                ("status", string("present")),
            ])
        }
        HostFact::Source(SourceFact::Missing) => object([("status", string("missing"))]),
        HostFact::Source(SourceFact::Unreadable { reason_code }) => object([
            ("reasonCode", string(reason_code)),
            ("status", string("unreadable")),
        ]),
        HostFact::Source(SourceFact::InvalidUtf8) => object([("status", string("invalid-utf8"))]),
        HostFact::Directory(DirectoryFact::Present(entries)) => object([
            (
                "entries",
                array(entries.iter().map(|entry| {
                    object([
                        (
                            "kind",
                            string(match entry.kind {
                                DirectoryEntryKind::File => "file",
                                DirectoryEntryKind::Directory => "directory",
                            }),
                        ),
                        ("name", string(&entry.name)),
                    ])
                })),
            ),
            ("status", string("present")),
        ]),
        HostFact::Directory(DirectoryFact::Missing) => object([("status", string("missing"))]),
        HostFact::Directory(DirectoryFact::Unreadable { reason_code }) => object([
            ("reasonCode", string(reason_code)),
            ("status", string("unreadable")),
        ]),
        HostFact::Containment(ContainmentFact::Inside { alias_class }) => object([
            ("aliasClass", string(alias_class)),
            ("status", string("inside")),
        ]),
        HostFact::Containment(ContainmentFact::Outside) => object([("status", string("outside"))]),
        HostFact::Containment(ContainmentFact::Missing) => object([("status", string("missing"))]),
        HostFact::Containment(ContainmentFact::Unresolved) => {
            object([("status", string("unresolved"))])
        }
    };
    object([
        ("kind", string(kind)),
        ("logicalPath", string(query.logical_path())),
        ("mountId", string(query.mount_id())),
        ("value", value),
    ])
}

pub(super) fn request_json(
    request: &KernelRequest,
    source_members: &BTreeMap<String, (String, String, u64)>,
) -> JsonValue {
    object([
        (
            "budgets",
            object([
                ("astNodes", unsigned(request.budgets().max_ast_nodes)),
                ("diagnostics", unsigned(request.budgets().max_diagnostics)),
                (
                    "generatedRustBytes",
                    unsigned(request.budgets().max_generated_rust_bytes),
                ),
                ("hirNodes", unsigned(request.budgets().max_hir_nodes)),
                (
                    "layoutTokens",
                    unsigned(request.budgets().max_layout_tokens),
                ),
                (
                    "loweredNodes",
                    unsigned(request.budgets().max_lowered_nodes),
                ),
                (
                    "projectionBytes",
                    unsigned(request.budgets().max_projection_bytes),
                ),
                ("rawTokens", unsigned(request.budgets().max_raw_tokens)),
                ("sourceFacts", unsigned(request.budgets().max_source_facts)),
                (
                    "sourceBytes",
                    unsigned(request.budgets().max_total_source_bytes),
                ),
            ]),
        ),
        ("entry", string(request.entry())),
        (
            "facts",
            array(
                request
                    .facts()
                    .iter()
                    .map(|(query, fact)| source_fact_json(query, fact, source_members)),
            ),
        ),
        (
            "languageMode",
            string(format!("topaz-{}", request.language_version().as_str())),
        ),
        (
            "mounts",
            array(request.mounts().iter().map(|mount| {
                object([
                    ("id", string(&mount.id)),
                    ("logicalRoot", string(&mount.logical_root)),
                ])
            })),
        ),
        (
            "package",
            object([
                (
                    "buildRole",
                    string(match request.package().build_role {
                        crate::BuildRole::Standalone => "standalone",
                        crate::BuildRole::Package => "package",
                    }),
                ),
                (
                    "capabilities",
                    array(request.package().capabilities.iter().map(string)),
                ),
                (
                    "dependencyMountIds",
                    array(request.package().dependency_mount_ids.iter().map(string)),
                ),
                ("deterministic", boolean(request.package().deterministic)),
                (
                    "executableProfile",
                    request
                        .package()
                        .executable_profile
                        .as_ref()
                        .map_or(JsonValue::Null, string),
                ),
                (
                    "externModules",
                    array(request.package().extern_modules.iter().map(string)),
                ),
                (
                    "generatedStdModules",
                    array(request.package().generated_std_modules.iter().map(
                        |(identity, module)| {
                            object([
                                ("identity", string(identity)),
                                ("path", string(&module.path)),
                                ("source", string(&module.source)),
                            ])
                        },
                    )),
                ),
                (
                    "identity",
                    request
                        .package()
                        .identity
                        .as_ref()
                        .map_or(JsonValue::Null, string),
                ),
                ("locked", boolean(request.package().locked)),
            ]),
        ),
        (
            "requestedSchemas",
            array(request.requested_schemas().iter().map(string)),
        ),
        ("schema", string(crate::REQUEST_SCHEMA)),
        (
            "terminalPhase",
            string(match request.terminal_phase() {
                crate::TerminalPhase::Tokens => "tokens",
                crate::TerminalPhase::Ast => "ast",
                crate::TerminalPhase::Resolved => "resolved",
                crate::TerminalPhase::Typed => "typed",
                crate::TerminalPhase::Lowered => "lowered",
                crate::TerminalPhase::RustSource => "rust-source",
            }),
        ),
    ])
}

fn provenance_json(unit: &KernelUnit) -> JsonValue {
    let runtime_templates = unit
        .lowered
        .as_ref()
        .map(|lowered| {
            lowered
                .runtime
                .templates
                .iter()
                .map(|template| {
                    object([
                        ("identity", string(&template.identity)),
                        ("sha256", string(&template.sha256)),
                    ])
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    provenance_json_with(&unit.provenance, runtime_templates)
}

pub(super) fn provenance_json_with(
    provenance: &crate::BootstrapProvenance,
    runtime_templates: Vec<JsonValue>,
) -> JsonValue {
    object([
        (
            "buildInputs",
            object([
                (
                    "bootstrapProfileSha256",
                    string(crate::build_provenance::BOOTSTRAP_PROFILE_SHA256),
                ),
                (
                    "buildProfile",
                    string(crate::build_provenance::BUILD_PROFILE),
                ),
                ("buildTarget", string(crate::build_provenance::BUILD_TARGET)),
                (
                    "cargoLockSha256",
                    string(crate::build_provenance::CARGO_LOCK_SHA256),
                ),
                (
                    "compilerSourceFiles",
                    array(crate::build_provenance::COMPILER_SOURCE_FILES.iter().map(
                        |(path, sha256, byte_length)| {
                            object([
                                ("byteLength", unsigned(*byte_length)),
                                ("path", string(*path)),
                                ("sha256", string(*sha256)),
                            ])
                        },
                    )),
                ),
                (
                    "compilerSourceSetId",
                    string(crate::build_provenance::COMPILER_SOURCE_SET_ID),
                ),
                ("runtimeTemplates", array(runtime_templates)),
                (
                    "rustToolchainSha256",
                    string(crate::build_provenance::RUST_TOOLCHAIN_SHA256),
                ),
                (
                    "schemaRegistrySha256",
                    string(crate::build_provenance::SCHEMA_REGISTRY_SHA256),
                ),
                (
                    "stage0Seed",
                    object([
                        (
                            "compilerSourceSetId",
                            string(crate::build_provenance::COMPILER_SOURCE_SET_ID),
                        ),
                        ("recoverySchema", string("topaz.stage0-recovery/v1")),
                    ]),
                ),
                (
                    "vendorPackages",
                    array(crate::build_provenance::VENDOR_PACKAGES.iter().map(
                        |(identity, sha256, file_count)| {
                            object([
                                ("fileCount", unsigned(*file_count)),
                                ("identity", string(*identity)),
                                ("sha256", string(*sha256)),
                            ])
                        },
                    )),
                ),
                (
                    "vendorSetId",
                    string(crate::build_provenance::VENDOR_SET_ID),
                ),
            ]),
        ),
        ("defaultEngine", string(provenance.default_engine)),
        ("engine", string(provenance.engine)),
        (
            "generatedSourceFixedPoint",
            string(fixed_point(provenance.generated_source_fixed_point)),
        ),
        ("languageMode", string(&provenance.language_mode)),
        (
            "nativeBinaryReproducibility",
            string(fixed_point(provenance.native_binary_reproducibility)),
        ),
        ("producerStage", unsigned(provenance.producer_stage.into())),
        ("productVersion", string(provenance.product_version)),
        ("resultStage", unsigned(provenance.result_stage.into())),
        ("schema", string(provenance.schema)),
        (
            "semanticFixedPoint",
            string(fixed_point(provenance.semantic_fixed_point)),
        ),
    ])
}

fn fixed_point(value: crate::FixedPointStatus) -> &'static str {
    match value {
        crate::FixedPointStatus::NotRun => "not-run",
        crate::FixedPointStatus::NotEstablished => "not-established",
        crate::FixedPointStatus::Pass => "pass",
        crate::FixedPointStatus::Fail => "fail",
        crate::FixedPointStatus::NotApplicable => "not-applicable",
    }
}

pub(super) fn root_digest(files: &BTreeMap<String, (String, Vec<u8>)>) -> String {
    let mut input = Vec::new();
    for (path, (_, bytes)) in files {
        input.extend_from_slice(path.as_bytes());
        input.push(0);
        input.extend_from_slice(sha256(bytes).as_bytes());
        input.push(0);
        input.extend_from_slice(bytes.len().to_string().as_bytes());
        input.push(b'\n');
    }
    sha256(&input)
}

pub(super) fn observation_files_root_digest(files: &BTreeMap<&str, &ObservationFile>) -> String {
    let mut input = Vec::new();
    for (path, file) in files {
        if *path == "topaz-observation.json" {
            continue;
        }
        input.extend_from_slice(path.as_bytes());
        input.push(0);
        input.extend_from_slice(sha256(&file.bytes).as_bytes());
        input.push(0);
        input.extend_from_slice(file.bytes.len().to_string().as_bytes());
        input.push(b'\n');
    }
    sha256(&input)
}

/// Builds the token-layer bundle from canonical module projections.
pub fn build_token_preview_observation(
    entry: &str,
    module: &str,
    source: &str,
    raw: &[CanonicalPreviewToken],
    layout: &[CanonicalPreviewToken],
    diagnostics: &[CanonicalPreviewDiagnostic],
) -> Result<ObservationBundle, String> {
    build_front_end_preview_observation(FrontEndPreviewInput {
        entry,
        module,
        source,
        raw,
        layout,
        ast: &[],
        diagnostics,
        terminal: crate::TerminalPhase::Tokens,
    })
}

/// Extends token projections through the canonical AST layer.
pub fn build_ast_preview_observation(
    entry: &str,
    module: &str,
    source: &str,
    raw: &[CanonicalPreviewToken],
    layout: &[CanonicalPreviewToken],
    ast: &[CanonicalPreviewAstNode],
    diagnostics: &[CanonicalPreviewDiagnostic],
) -> Result<ObservationBundle, String> {
    build_front_end_preview_observation(FrontEndPreviewInput {
        entry,
        module,
        source,
        raw,
        layout,
        ast,
        diagnostics,
        terminal: crate::TerminalPhase::Ast,
    })
}

pub(super) fn preview_token_rows(
    identity: &SourceIdentity,
    source: &str,
    raw: &[CanonicalPreviewToken],
    layout: &[CanonicalPreviewToken],
) -> Result<Vec<JsonValue>, String> {
    let mut rows = Vec::with_capacity(raw.len() + layout.len());
    for (stream, values) in [("raw", raw), ("layout", layout)] {
        for (ordinal, value) in values.iter().enumerate() {
            if value.lo > value.hi || value.hi as usize > source.len() {
                return Err(format!(
                    "{stream} token {ordinal} has an out-of-range byte span {}..{}",
                    value.lo, value.hi
                ));
            }
            let spelling = source
                .get(value.lo as usize..value.hi as usize)
                .ok_or_else(|| format!("{stream} token {ordinal} splits a UTF-8 scalar"))?;
            if ordinal > 0 {
                let previous = &values[ordinal - 1];
                if value.lo < previous.lo {
                    return Err(format!("{stream} token {ordinal} is not in source order"));
                }
            }
            rows.push(object([
                ("kind", string(&value.kind)),
                ("ordinal", unsigned(ordinal as u64)),
                ("schema", string(TOKENS_SCHEMA)),
                ("sourceId", string(&identity.source_id)),
                ("sourceOrdinal", unsigned(identity.ordinal)),
                (
                    "span",
                    object([
                        ("hi", unsigned(value.hi.into())),
                        ("lo", unsigned(value.lo.into())),
                        ("sourceId", string(&identity.source_id)),
                    ]),
                ),
                ("spelling", string(spelling)),
                ("stream", string(stream)),
                ("synthetic", boolean(value.synthetic)),
            ]));
        }
    }
    Ok(rows)
}

pub(super) fn preview_ast_rows(
    identity: &SourceIdentity,
    source: &str,
    ast: &[CanonicalPreviewAstNode],
) -> Result<Vec<JsonValue>, String> {
    let spelling_kinds = [
        "identifier",
        "expression/integer",
        "expression/float",
        "expression/duration",
        "expression/boolean",
        "expression/null",
        "expression/unit",
        "expression/string",
        "expression/template",
        "expression/identifier",
        "pattern/wildcard",
        "template-tag",
        "string-part/text",
        "type/literal",
        "type/unit",
    ];
    let allowed_attributes = [
        "collection",
        "exported",
        "floatBits",
        "inclusive",
        "mutable",
        "operator",
        "unit",
        "value",
        "valueDecimal",
        "variadic",
    ];
    let mut rows = Vec::with_capacity(ast.len());
    for (ordinal, node) in ast.iter().enumerate() {
        if node.lo > node.hi || node.hi as usize > source.len() {
            return Err(format!(
                "AST node {ordinal} has an out-of-range byte span {}..{}",
                node.lo, node.hi
            ));
        }
        let source_spelling = source
            .get(node.lo as usize..node.hi as usize)
            .ok_or_else(|| format!("AST node {ordinal} splits a UTF-8 scalar"))?;
        match (ordinal, node.parent) {
            (0, None) if node.kind == "program" && node.field == "root" && node.index == 0 => {}
            (0, _) => {
                return Err(
                    "AST node 0 must be the parentless program root in field `root`".to_string(),
                );
            }
            (_, Some(parent)) if (parent as usize) < ordinal => {}
            _ => {
                return Err(format!("AST node {ordinal} parent must precede the child"));
            }
        }
        let node_id = format!("{}#n{ordinal:08x}", identity.source_id);
        let mut fields = BTreeMap::<String, JsonValue>::from([
            ("field".to_string(), string(&node.field)),
            ("index".to_string(), unsigned(node.index)),
            ("kind".to_string(), string(&node.kind)),
            ("nodeId".to_string(), string(&node_id)),
            (
                "parentNodeId".to_string(),
                node.parent.map_or(JsonValue::Null, |parent| {
                    string(format!("{}#n{parent:08x}", identity.source_id))
                }),
            ),
            ("schema".to_string(), string(AST_SCHEMA)),
            ("sourceId".to_string(), string(&identity.source_id)),
            (
                "span".to_string(),
                object([
                    ("hi", unsigned(node.hi.into())),
                    ("lo", unsigned(node.lo.into())),
                    ("sourceId", string(&identity.source_id)),
                ]),
            ),
            (
                "spelling".to_string(),
                string(if spelling_kinds.contains(&node.kind.as_str()) {
                    source_spelling
                } else {
                    ""
                }),
            ),
        ]);
        let mut names = BTreeSet::new();
        for attribute in &node.attributes {
            if !allowed_attributes.contains(&attribute.name.as_str()) {
                return Err(format!(
                    "AST node {ordinal} has unknown attribute `{}`",
                    attribute.name
                ));
            }
            if !names.insert(attribute.name.as_str()) {
                return Err(format!(
                    "AST node {ordinal} repeats attribute `{}`",
                    attribute.name
                ));
            }
            let value = match (&attribute.name[..], &attribute.value) {
                ("floatBits", CanonicalPreviewAstValue::Null)
                    if node.kind == "expression/float" =>
                {
                    source_spelling
                        .parse::<f64>()
                        .ok()
                        .filter(|value| value.is_finite())
                        .map(|value| string(format!("{:016x}", value.to_bits())))
                        .unwrap_or(JsonValue::Null)
                }
                (_, CanonicalPreviewAstValue::String(value)) => string(value),
                (_, CanonicalPreviewAstValue::Bool(value)) => boolean(*value),
                (_, CanonicalPreviewAstValue::Null) => JsonValue::Null,
            };
            fields.insert(attribute.name.clone(), value);
        }
        rows.push(object(fields));
    }
    Ok(rows)
}

struct FrontEndPreviewInput<'input> {
    entry: &'input str,
    module: &'input str,
    source: &'input str,
    raw: &'input [CanonicalPreviewToken],
    layout: &'input [CanonicalPreviewToken],
    ast: &'input [CanonicalPreviewAstNode],
    diagnostics: &'input [CanonicalPreviewDiagnostic],
    terminal: crate::TerminalPhase,
}

fn build_front_end_preview_observation(
    input: FrontEndPreviewInput<'_>,
) -> Result<ObservationBundle, String> {
    let FrontEndPreviewInput {
        entry,
        module,
        source,
        raw,
        layout,
        ast,
        diagnostics,
        terminal,
    } = input;
    validate_schema_registry()?;
    let budgets = crate::ResourceBudgets::default();
    for (label, observed, limit) in [
        (
            "source-byte",
            source.len() as u64,
            budgets.max_total_source_bytes,
        ),
        ("raw-token", raw.len() as u64, budgets.max_raw_tokens),
        (
            "layout-token",
            layout.len() as u64,
            budgets.max_layout_tokens,
        ),
        ("ast-node", ast.len() as u64, budgets.max_ast_nodes),
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
    let identity = SourceIdentity {
        source_id: source_id(module, entry),
        module: module.to_string(),
        path: topaz_resolve::normalize_path(entry),
        ordinal: 0,
    };
    let member = "sources/000000.tpz".to_string();
    let source_digest = sha256(source.as_bytes());
    let source_members = BTreeMap::from([(
        identity.path.clone(),
        (member.clone(), source_digest.clone(), source.len() as u64),
    )]);
    let mut files = BTreeMap::<String, (String, Vec<u8>)>::new();
    files.insert(
        member.clone(),
        ("topaz.source/utf8".to_string(), source.as_bytes().to_vec()),
    );
    files.insert(
        "source-set.jsonl".to_string(),
        (
            SOURCE_SET_SCHEMA.to_string(),
            encode_jsonl(&[object([
                ("byteLength", unsigned(source.len() as u64)),
                ("contentSha256", string(&source_digest)),
                ("entry", boolean(true)),
                ("member", string(&member)),
                ("module", string(&identity.module)),
                ("originRole", string("source")),
                ("path", string(&identity.path)),
                ("rowKind", string("source")),
                ("schema", string(SOURCE_SET_SCHEMA)),
                ("sourceId", string(&identity.source_id)),
                ("sourceOrdinal", unsigned(0)),
            ])]),
        ),
    );
    let token_values = preview_token_rows(&identity, source, raw, layout)?;
    files.insert(
        "tokens.jsonl".to_string(),
        (TOKENS_SCHEMA.to_string(), encode_jsonl(&token_values)),
    );
    let ast_values = preview_ast_rows(&identity, source, ast)?;
    files.insert(
        "ast.jsonl".to_string(),
        (
            AST_SCHEMA.to_string(),
            if terminal >= crate::TerminalPhase::Ast {
                encode_jsonl(&ast_values)
            } else {
                Vec::new()
            },
        ),
    );
    files.insert(
        "resolved.jsonl".to_string(),
        (RESOLVED_SCHEMA.to_string(), Vec::new()),
    );
    for (ordinal, value) in diagnostics.iter().enumerate() {
        if value.lo > value.hi || value.hi as usize > source.len() {
            return Err(format!(
                "diagnostic {ordinal} has an out-of-range byte span {}..{}",
                value.lo, value.hi
            ));
        }
        source
            .get(value.lo as usize..value.hi as usize)
            .ok_or_else(|| format!("diagnostic {ordinal} splits a UTF-8 scalar"))?;
    }
    let diagnostic_values = diagnostics
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let primary_message = match value.code.as_str() {
                "TPZ0003" => "string starts here",
                "TPZ0006" => "must start with the whitespace prefix of the closing `\"\"\"`",
                _ => "",
            };
            object([
                ("code", string(&value.code)),
                ("message", string(&value.message)),
                ("notes", array(value.notes.iter().map(string))),
                ("ordinal", unsigned(ordinal as u64)),
                (
                    "primary",
                    object([
                        ("message", string(primary_message)),
                        (
                            "span",
                            object([
                                ("hi", unsigned(value.hi.into())),
                                ("lo", unsigned(value.lo.into())),
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
            ])
        })
        .collect::<Vec<_>>();
    files.insert(
        "diagnostics.jsonl".to_string(),
        (
            DIAGNOSTICS_SCHEMA.to_string(),
            if diagnostic_values.is_empty() {
                Vec::new()
            } else {
                encode_jsonl(&diagnostic_values)
            },
        ),
    );

    let request = crate::KernelRequest::checked(
        &identity.path,
        Some(""),
        topaz_syntax::LangVersion::CURRENT,
        crate::PackageFacts::standalone(),
    )
    .with_terminal_phase(terminal);
    let request_bytes = encode(&request_json(&request, &source_members));
    let mut request_material = request_bytes.clone();
    request_material.extend_from_slice(member.as_bytes());
    request_material.push(0);
    request_material.extend_from_slice(source.as_bytes());
    let request_digest = sha256(&request_material);
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
    let status = if diagnostics.is_empty() {
        "completed"
    } else {
        "rejected"
    };
    let response = encode(&object([
        (
            "highestCompletedPhase",
            string(if terminal >= crate::TerminalPhase::Ast {
                "ast"
            } else {
                "tokens"
            }),
        ),
        (
            "phases",
            object([
                (
                    "ast",
                    string(if terminal >= crate::TerminalPhase::Ast {
                        "produced"
                    } else {
                        "not-requested"
                    }),
                ),
                ("lowered", string("not-requested")),
                ("resolved", string("not-requested")),
                ("rustSource", string("not-requested")),
                ("tokens", string("produced")),
                ("typed", string("not-requested")),
            ]),
        ),
        (
            "projectionDigests",
            object([
                (
                    "ast",
                    if terminal >= crate::TerminalPhase::Ast {
                        string(sha256(&files["ast.jsonl"].1))
                    } else {
                        JsonValue::Null
                    },
                ),
                ("diagnostics", string(sha256(&files["diagnostics.jsonl"].1))),
                ("lowered", JsonValue::Null),
                ("resolved", JsonValue::Null),
                ("rustSource", JsonValue::Null),
                ("sourceSet", string(sha256(&files["source-set.jsonl"].1))),
                ("tokens", string(sha256(&files["tokens.jsonl"].1))),
                ("typed", JsonValue::Null),
            ]),
        ),
        ("requestDigest", string(request_digest)),
        ("schema", string(crate::RESPONSE_SCHEMA)),
        ("status", string(status)),
    ]));
    files.insert(
        "response.json".to_string(),
        (crate::RESPONSE_SCHEMA.to_string(), response),
    );
    finish_observation(files, budgets.max_projection_bytes, true)
}

pub(super) fn finish_observation(
    mut files: BTreeMap<String, (String, Vec<u8>)>,
    max_projection_bytes: u64,
    validate_bundle: bool,
) -> Result<ObservationBundle, String> {
    let digest = root_digest(&files);
    let entries = files
        .iter()
        .map(|(path, (schema, bytes))| {
            object([
                ("byteLength", unsigned(bytes.len() as u64)),
                ("path", string(path)),
                ("schema", string(schema)),
                ("sha256", string(sha256(bytes))),
            ])
        })
        .collect::<Vec<_>>();
    files.insert(
        "topaz-observation.json".to_string(),
        (
            BUNDLE_SCHEMA.to_string(),
            encode(&object([
                ("files", array(entries)),
                ("rootDigest", string(digest)),
                ("schema", string(BUNDLE_SCHEMA)),
            ])),
        ),
    );
    let projection_bytes = files
        .values()
        .map(|(_, bytes)| bytes.len() as u64)
        .sum::<u64>();
    if projection_bytes > max_projection_bytes {
        return Err(format!(
            "projection-byte resource limit: observed {projection_bytes}, limit {max_projection_bytes}"
        ));
    }
    let bundle = ObservationBundle {
        files: files
            .into_iter()
            .map(|(path, (schema, bytes))| ObservationFile {
                path,
                schema,
                bytes,
            })
            .collect(),
    };
    if validate_bundle {
        bundle.validate()?;
    }
    Ok(bundle)
}

/// Omits product rows beyond the terminal phase selected by the request.
pub fn build_observation(execution: &KernelExecution) -> Result<ObservationBundle, String> {
    validate_schema_registry()?;
    let (unit, status) = match &execution.outcome {
        KernelOutcome::Completed(unit) => (unit.as_ref(), "completed"),
        KernelOutcome::Rejected(unit) => (unit.as_ref(), "rejected"),
        KernelOutcome::NeedHostFacts(_) => {
            return Err("cannot observe an incomplete host-fact request".to_string());
        }
        KernelOutcome::Declined { .. }
        | KernelOutcome::ResourceLimit(_)
        | KernelOutcome::CompilerFault { .. } => {
            return Err("cannot observe a non-front-end terminal outcome".to_string());
        }
    };
    let sources = source_identities(unit);
    let mut source_members = BTreeMap::new();
    let mut files = BTreeMap::<String, (String, Vec<u8>)>::new();
    let mut source_rows = Vec::new();
    let mut token_rows_all = Vec::new();
    let mut ast_rows = Vec::new();
    let mut identity_nodes = BTreeMap::<(u32, u32, u32), String>::new();

    let mut ordered_sources: Vec<(u32, &SourceIdentity)> = sources
        .iter()
        .map(|(file, identity)| (*file, identity))
        .collect();
    ordered_sources.sort_by_key(|(_, identity)| identity.ordinal);
    for (file, identity) in ordered_sources {
        let module = unit
            .resolved
            .modules
            .iter()
            .find(|module| module.file.0 == file);
        let src = unit.resolved.map.file(topaz_diag::FileId(file)).src();
        let member = format!("sources/{:06}.tpz", identity.ordinal);
        files.insert(
            member.clone(),
            ("topaz.source/utf8".to_string(), src.as_bytes().to_vec()),
        );
        let digest = sha256(src.as_bytes());
        source_members.insert(
            identity.path.clone(),
            (member.clone(), digest.clone(), src.len() as u64),
        );
        source_rows.push(object([
            ("byteLength", unsigned(src.len() as u64)),
            ("contentSha256", string(digest)),
            (
                "entry",
                boolean(module.is_some_and(|module| module.is_entry)),
            ),
            ("member", string(member)),
            ("module", string(&identity.module)),
            (
                "originRole",
                string(match module {
                    Some(module) if module.is_extern => "extern",
                    Some(_) => "source",
                    None => "diagnostic",
                }),
            ),
            ("path", string(&identity.path)),
            ("rowKind", string("source")),
            ("schema", string(SOURCE_SET_SCHEMA)),
            ("sourceId", string(&identity.source_id)),
            ("sourceOrdinal", unsigned(identity.ordinal)),
        ]));
        if let Some(module) = module {
            token_rows_all.extend(token_rows(identity, src, "raw", &module.raw_tokens));
            token_rows_all.extend(token_rows(identity, src, "layout", &module.layout_tokens));
            let mut projector = AstProjector::new(&identity.source_id, src);
            projector.program(&module.program);
            identity_nodes.extend(
                projector
                    .identity_nodes
                    .iter()
                    .map(|((lo, hi), node)| ((file, *lo, *hi), node.clone())),
            );
            ast_rows.extend(projector.rows);
        }
    }
    for (query, fact) in execution.request.facts() {
        let (
            crate::HostQuery::ReadSource { logical_path, .. },
            crate::HostFact::Source(crate::SourceFact::Present(source)),
        ) = (query, fact)
        else {
            continue;
        };
        if source_members.contains_key(logical_path) {
            continue;
        }
        let ordinal = source_members.len() as u64;
        let member = format!("sources/{ordinal:06}.tpz");
        let digest = sha256(source.as_bytes());
        let identity = source_id("queried-source", logical_path);
        files.insert(
            member.clone(),
            ("topaz.source/utf8".to_string(), source.as_bytes().to_vec()),
        );
        source_members.insert(
            logical_path.clone(),
            (member.clone(), digest.clone(), source.len() as u64),
        );
        source_rows.push(object([
            ("byteLength", unsigned(source.len() as u64)),
            ("contentSha256", string(digest)),
            ("entry", boolean(false)),
            ("member", string(member)),
            ("module", string("queried-source")),
            ("originRole", string("queried-source")),
            ("path", string(logical_path)),
            ("rowKind", string("source")),
            ("schema", string(SOURCE_SET_SCHEMA)),
            ("sourceId", string(identity)),
            ("sourceOrdinal", unsigned(ordinal)),
        ]));
    }
    for (query, fact) in execution.request.facts() {
        let JsonValue::Object(fields) = source_fact_json(query, fact, &source_members) else {
            unreachable!("host fact projection is an object");
        };
        let mut fields = fields
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
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
        (TOKENS_SCHEMA.to_string(), encode_jsonl(&token_rows_all)),
    );
    files.insert(
        "ast.jsonl".to_string(),
        (AST_SCHEMA.to_string(), encode_jsonl(&ast_rows)),
    );
    let resolved = resolved_rows(unit, &sources, &identity_nodes);
    files.insert(
        "resolved.jsonl".to_string(),
        (RESOLVED_SCHEMA.to_string(), encode_jsonl(&resolved)),
    );
    let terminal = execution.request.terminal_phase();
    let typed_requested = terminal >= crate::TerminalPhase::Typed;
    let lowered_requested = terminal >= crate::TerminalPhase::Lowered;
    let rust_requested = terminal >= crate::TerminalPhase::RustSource;
    let typed_completed = typed_requested && unit.checked.is_some();
    let lowered_completed = lowered_requested && unit.lowered.is_some();
    let rust_completed = rust_requested && unit.rust_source.is_some();
    if typed_requested {
        let typed = typed_rows(unit, &sources, &identity_nodes);
        files.insert(
            "typed.jsonl".to_string(),
            (
                TYPED_SCHEMA.to_string(),
                if typed.is_empty() {
                    Vec::new()
                } else {
                    encode_jsonl(&typed)
                },
            ),
        );
    }
    if lowered_requested {
        let lowered = lowered_rows(unit, &sources);
        files.insert(
            "lowered.jsonl".to_string(),
            (
                LOWERED_SCHEMA.to_string(),
                if lowered.is_empty() {
                    Vec::new()
                } else {
                    encode_jsonl(&lowered)
                },
            ),
        );
    }
    if rust_requested {
        let rows = unit.rust_source.as_ref().map_or_else(Vec::new, |source| {
            vec![object([
                ("byteLength", unsigned(source.len() as u64)),
                ("rowKind", string("generated-source")),
                ("schema", string(RUST_SOURCE_SCHEMA)),
                ("sha256", string(sha256(source.as_bytes()))),
                ("source", string(source)),
            ])]
        });
        files.insert(
            "rust-source.jsonl".to_string(),
            (
                RUST_SOURCE_SCHEMA.to_string(),
                if rows.is_empty() {
                    Vec::new()
                } else {
                    encode_jsonl(&rows)
                },
            ),
        );
    }
    let mut all_diagnostics = unit.resolved.diagnostics.clone();
    if typed_requested && let Some(checked) = &unit.checked {
        all_diagnostics.extend(checked.diagnostics.clone());
    }
    let diagnostics = diagnostic_rows(&all_diagnostics, &sources);
    files.insert(
        "diagnostics.jsonl".to_string(),
        (
            DIAGNOSTICS_SCHEMA.to_string(),
            if diagnostics.is_empty() {
                Vec::new()
            } else {
                encode_jsonl(&diagnostics)
            },
        ),
    );
    let request = encode(&request_json(&execution.request, &source_members));
    let request_digest = {
        let mut material = request.clone();
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
        (crate::REQUEST_SCHEMA.to_string(), request),
    );
    let provenance = encode(&provenance_json(unit));
    files.insert(
        "provenance.json".to_string(),
        (crate::PROVENANCE_SCHEMA.to_string(), provenance),
    );
    let response = encode(&object([
        (
            "highestCompletedPhase",
            string(if rust_completed {
                "rust-source"
            } else if lowered_completed {
                "lowered"
            } else if typed_completed {
                "typed"
            } else {
                "resolved"
            }),
        ),
        (
            "phases",
            object([
                ("ast", string("produced")),
                (
                    "lowered",
                    string(if lowered_completed {
                        "produced"
                    } else if lowered_requested {
                        "blocked"
                    } else {
                        "not-requested"
                    }),
                ),
                ("resolved", string("produced")),
                (
                    "rustSource",
                    string(if rust_completed {
                        "produced"
                    } else if rust_requested {
                        "blocked"
                    } else {
                        "not-requested"
                    }),
                ),
                ("tokens", string("produced")),
                (
                    "typed",
                    string(if typed_completed {
                        "produced"
                    } else if typed_requested {
                        "blocked"
                    } else {
                        "not-requested"
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
                    if lowered_requested {
                        string(sha256(&files["lowered.jsonl"].1))
                    } else {
                        JsonValue::Null
                    },
                ),
                ("resolved", string(sha256(&files["resolved.jsonl"].1))),
                (
                    "rustSource",
                    if rust_requested {
                        string(sha256(&files["rust-source.jsonl"].1))
                    } else {
                        JsonValue::Null
                    },
                ),
                ("sourceSet", string(sha256(&files["source-set.jsonl"].1))),
                ("tokens", string(sha256(&files["tokens.jsonl"].1))),
                (
                    "typed",
                    if typed_requested {
                        string(sha256(&files["typed.jsonl"].1))
                    } else {
                        JsonValue::Null
                    },
                ),
            ]),
        ),
        ("requestDigest", string(request_digest)),
        ("schema", string(crate::RESPONSE_SCHEMA)),
        ("status", string(status)),
    ]));
    files.insert(
        "response.json".to_string(),
        (crate::RESPONSE_SCHEMA.to_string(), response),
    );

    finish_observation(
        files,
        execution.request.budgets().max_projection_bytes,
        false,
    )
}
