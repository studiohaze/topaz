use crate::*;

/// Preserves raw and layout streams separately so synthetic tokens remain visible.
pub struct TokenPreviewResult {
    pub raw: Vec<topaz_kernel::CanonicalPreviewToken>,
    pub layout: Vec<topaz_kernel::CanonicalPreviewToken>,
    pub ast: Vec<topaz_kernel::CanonicalPreviewAstNode>,
    pub diagnostics: Vec<topaz_kernel::CanonicalPreviewDiagnostic>,
}

/// Retains request and round state so observations need not repeat the fact loop.
pub struct ResolvedPreviewResult {
    pub request: topaz_kernel::KernelRequest,
    pub modules: Vec<topaz_kernel::CanonicalPreviewModule>,
    pub edges: Vec<topaz_kernel::CanonicalPreviewImportEdge>,
    pub scopes: Vec<topaz_kernel::CanonicalPreviewResolvedScope>,
    pub declarations: Vec<topaz_kernel::CanonicalPreviewResolvedDeclaration>,
    pub references: Vec<topaz_kernel::CanonicalPreviewResolvedReference>,
    pub exports: Vec<topaz_kernel::CanonicalPreviewResolvedExport>,
    pub diagnostics: Vec<topaz_kernel::CanonicalPreviewResolvedDiagnostic>,
    pub rounds: u64,
    pub(crate) checker: Option<CheckerPreviewProjection>,
}

impl ResolvedPreviewResult {
    /// Borrows the rows required by the kernel's resolved observation builder.
    pub fn observation_input(&self) -> topaz_kernel::ResolvedPreviewObservationInput<'_> {
        topaz_kernel::ResolvedPreviewObservationInput {
            request: &self.request,
            modules: &self.modules,
            edges: &self.edges,
            scopes: &self.scopes,
            declarations: &self.declarations,
            references: &self.references,
            exports: &self.exports,
            diagnostics: &self.diagnostics,
        }
    }
}

pub(crate) struct CheckerPreviewProjection {
    pub(crate) nodes: Vec<topaz_hir::TypedNode>,
    pub(crate) calls: Vec<topaz_hir::TypedCall>,
    pub(crate) captures: Vec<topaz_hir::TypedCapture>,
    pub(crate) diagnostics: Vec<topaz_kernel::CanonicalPreviewCheckDiagnostic>,
}

/// Runs raw, layout, and parse preview through a reusable front-end session.
pub fn preview_source_with(
    session: &FrontEndSession,
    source_id: &str,
    source: &str,
) -> Result<TokenPreviewResult, String> {
    let source_id_json =
        json_stringify(&Value::str(source_id), true).map_err(|error| error.to_string())?;
    let source_json =
        json_stringify(&Value::str(source), true).map_err(|error| error.to_string())?;
    let request = format!(
        concat!(
            "{{\"schema\":\"{schema}\",\"terminal\":\"ast\",",
            "\"entry\":{source_id},\"root\":\"\",\"source\":{source},",
            "\"sourceId\":{source_id},\"facts\":[],",
            "\"package\":{{\"buildRole\":\"standalone\",\"externModules\":[],",
            "\"externReplayModules\":[],\"externReplayErrors\":[],",
            "\"generatedStdModules\":[]}},",
            "\"maxAstNodes\":{max_nodes},\"maxAstDepth\":{max_depth}}}"
        ),
        schema = EXCHANGE_SCHEMA,
        source_id = source_id_json,
        source = source_json,
        max_nodes = MAX_AST_NODES,
        max_depth = MAX_AST_DEPTH,
    );
    let response = session.invoke(request.as_bytes())?;
    let root = decode_front_end_response_bytes(&response, "front-end preview response")?;
    expect_json_string(&root, "sourceId", source_id)?;
    let raw = parse_tokens(&root, "raw", "raw")?;
    let layout = parse_tokens(&root, "layout", "layout")?;
    let ast = parse_ast(&root)?;
    let diagnostics = parse_diagnostics(&root)?;
    let status = json_string_field(&root, "status")?;
    if (diagnostics.is_empty() && status != "completed")
        || (!diagnostics.is_empty() && status != "rejected")
    {
        return Err("front-end preview response status contradicts diagnostics".to_string());
    }
    Ok(TokenPreviewResult {
        raw,
        layout,
        ast,
        diagnostics,
    })
}

/// Previews one source string with a freshly admitted embedded compiler session.
pub fn preview_source(source_id: &str, source: &str) -> Result<TokenPreviewResult, String> {
    preview_source_with(&FrontEndSession::new()?, source_id, source)
}

pub(crate) fn preview_fact(
    source: &dyn topaz_kernel::HostFactSource,
    request: &topaz_kernel::KernelRequest,
    query: &topaz_kernel::HostQuery,
) -> topaz_kernel::HostFact {
    if let topaz_kernel::HostQuery::ReadSource { logical_path, .. } = query
        && let Some(path) = logical_path.strip_suffix(".tpz")
    {
        let segments = path.split('/').collect::<Vec<_>>();
        if segments.first() == Some(&"std")
            && let Some((_, module)) = topaz_resolve::std_module_source(&segments)
        {
            return topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Present(
                module.to_string(),
            ));
        }
    }
    source.respond(request, query)
}

pub(crate) fn enforce_resolver_budgets(
    request: &topaz_kernel::KernelRequest,
    modules: &[topaz_kernel::CanonicalPreviewModule],
    scopes: &[topaz_kernel::CanonicalPreviewResolvedScope],
    declarations: &[topaz_kernel::CanonicalPreviewResolvedDeclaration],
    references: &[topaz_kernel::CanonicalPreviewResolvedReference],
    exports: &[topaz_kernel::CanonicalPreviewResolvedExport],
    diagnostics: &[topaz_kernel::CanonicalPreviewResolvedDiagnostic],
) -> Result<(), String> {
    let budgets = request.budgets();
    let source_facts = request
        .facts()
        .values()
        .filter(|fact| matches!(fact, topaz_kernel::HostFact::Source(_)))
        .count() as u64;
    let source_bytes = request
        .facts()
        .values()
        .filter_map(|fact| match fact {
            topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Present(source)) => {
                Some(source.len() as u64)
            }
            _ => None,
        })
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
    let resolved_facts =
        (scopes.len() + declarations.len() + references.len() + exports.len()) as u64;
    for (label, observed, limit) in [
        ("source-fact", source_facts, budgets.max_source_facts),
        (
            "total-source-byte",
            source_bytes,
            budgets.max_total_source_bytes,
        ),
        ("raw-token", raw_tokens, budgets.max_raw_tokens),
        ("layout-token", layout_tokens, budgets.max_layout_tokens),
        ("ast-node", ast_nodes, budgets.max_ast_nodes),
        ("resolved-fact", resolved_facts, budgets.max_hir_nodes),
        (
            "diagnostic",
            diagnostics.len() as u64,
            budgets.max_diagnostics,
        ),
    ] {
        if observed > limit {
            return Err(format!(
                "front-end resolver preview {label} resource limit: observed {observed}, limit {limit}"
            ));
        }
    }
    Ok(())
}

pub(crate) fn preview_resolved_or_typed_with(
    session: &FrontEndSession,
    source: &dyn topaz_kernel::HostFactSource,
    mut request: topaz_kernel::KernelRequest,
) -> Result<ResolvedPreviewResult, String> {
    let requested_terminal = match request.terminal_phase() {
        topaz_kernel::TerminalPhase::Resolved => "resolved",
        topaz_kernel::TerminalPhase::Typed => "typed",
        _ => {
            return Err(
                "front-end preview fact loop requires the resolved or typed terminal phase"
                    .to_string(),
            );
        }
    };
    let terminal = requested_terminal;
    let max_rounds = request
        .budgets()
        .max_source_facts
        .saturating_mul(3)
        .saturating_add(4);
    let mut rounds = 0u64;
    loop {
        if rounds >= max_rounds {
            return Err(format!(
                "front-end resolver preview fact rounds exceed {max_rounds}"
            ));
        }
        rounds += 1;
        let encoded = encode_preview_request(&request, terminal)?;
        let response = session.invoke(&encoded)?;
        let root = decode_front_end_response_bytes(&response, "front-end resolver response")?;
        let status = json_string_field(&root, "status")?;
        let queries = parse_queries(&root)?;
        if advance_compiler_fact_round(source, &mut request, status, queries, "front-end resolver")?
        {
            continue;
        }
        if status != "completed" && status != "rejected" {
            return Err(format!(
                "front-end resolver returned invalid status `{status}`"
            ));
        }
        let modules = parse_modules(&root)?;
        let edges = parse_edges(&root)?;
        let scopes = parse_scopes(&root)?;
        let declarations = parse_declarations(&root)?;
        let references = parse_references(&root)?;
        let exports = parse_exports(&root)?;
        let diagnostics = parse_resolved_diagnostics(&root)?;
        let checker_diagnostics = parse_checker_diagnostics(&root)?;
        let checker = if terminal == "typed" {
            let (nodes, calls, captures) =
                if diagnostics.is_empty() && checker_diagnostics.is_empty() {
                    (
                        parse_typed_nodes(&root, &modules, request.language_version())?,
                        parse_typed_calls(&root, &modules, request.language_version())?,
                        parse_typed_captures(&root, &modules, request.language_version())?,
                    )
                } else {
                    (Vec::new(), Vec::new(), Vec::new())
                };
            Some(CheckerPreviewProjection {
                nodes,
                calls,
                captures,
                diagnostics: checker_diagnostics,
            })
        } else {
            if !checker_diagnostics.is_empty()
                || !json_array_field(&root, "typedNodes")?.is_empty()
                || !json_array_field(&root, "typedCalls")?.is_empty()
                || !json_array_field(&root, "typedCaptures")?.is_empty()
            {
                return Err(
                    "front-end resolver response unexpectedly contains checker output".to_string(),
                );
            }
            None
        };
        enforce_resolver_budgets(
            &request,
            &modules,
            &scopes,
            &declarations,
            &references,
            &exports,
            &diagnostics,
        )?;
        let rejected = !diagnostics.is_empty()
            || checker
                .as_ref()
                .is_some_and(|projection| !projection.diagnostics.is_empty());
        if (status == "completed" && rejected) || (status == "rejected" && !rejected) {
            return Err("front-end preview status contradicts diagnostics".to_string());
        }
        return Ok(ResolvedPreviewResult {
            request,
            modules,
            edges,
            scopes,
            declarations,
            references,
            exports,
            diagnostics,
            rounds,
            checker,
        });
    }
}

/// Resolves a fact-backed package through a reusable embedded session.
pub fn preview_resolved_with(
    session: &FrontEndSession,
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<ResolvedPreviewResult, String> {
    if request.terminal_phase() != topaz_kernel::TerminalPhase::Resolved {
        return Err("front-end resolver preview requires the resolved terminal phase".to_string());
    }
    preview_resolved_or_typed_with(session, source, request)
}

/// Resolves a fact-backed package with a fresh embedded compiler session.
pub fn preview_resolved(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
) -> Result<ResolvedPreviewResult, String> {
    preview_resolved_with(&FrontEndSession::new()?, source, request)
}
