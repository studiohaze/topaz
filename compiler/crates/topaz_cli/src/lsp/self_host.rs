use crate::*;

pub(super) struct LspWorkspace {
    pub(super) target: PackageTarget,
    pub(super) logical_root: PathBuf,
}

pub(super) struct LspPublishedDiagnostic {
    pub(super) diagnostic: Diagnostic,
    pub(super) profile_rule: Option<&'static str>,
}

pub(super) struct SelfLspSnapshot {
    pub(super) typed: topaz_self_frontend::TypedPreviewResult,
}

pub(super) struct SelfLspSession {
    pub(super) check_profile: Option<profile::CheckProfile>,
    pub(super) active_uri: Option<String>,
    pub(super) snapshot: Option<SelfLspSnapshot>,
}

impl SelfLspSession {
    pub(super) fn new() -> Result<Self, String> {
        let identity = topaz_self_frontend::installed_stage2_identity()?;
        if identity.producer != "topaz-stage2" || identity.producer_stage != 2 {
            return Err("self LSP requires the exact installed stage-2 compiler image".to_string());
        }
        Ok(Self {
            check_profile: None,
            active_uri: None,
            snapshot: None,
        })
    }

    pub(super) fn set_check_profile(&mut self, check_profile: Option<profile::CheckProfile>) {
        if self.check_profile != check_profile {
            self.check_profile = check_profile;
            self.invalidate();
        }
    }

    pub(super) fn invalidate(&mut self) {
        self.active_uri = None;
        self.snapshot = None;
    }

    pub(super) fn snapshot<'a>(
        &'a mut self,
        uri: &str,
        text: &str,
        version: LangVersion,
        workspace: Option<&LspWorkspace>,
        documents: &BTreeMap<String, String>,
    ) -> Result<&'a SelfLspSnapshot, String> {
        if self.active_uri.as_deref() != Some(uri) || self.snapshot.is_none() {
            self.snapshot = Some(compile_self_lsp_snapshot(
                uri,
                text,
                version,
                workspace,
                documents,
                self.check_profile,
            )?);
            self.active_uri = Some(uri.to_string());
        }
        self.snapshot
            .as_ref()
            .ok_or_else(|| "self LSP snapshot cache is empty".to_string())
    }
}

pub(super) struct LspSelfFactHost<'a> {
    pub(super) package: Option<PackageFactHost<'a>>,
    pub(super) overlays: BTreeMap<String, String>,
}

impl topaz_kernel::HostFactSource for LspSelfFactHost<'_> {
    fn respond(
        &self,
        request: &topaz_kernel::KernelRequest,
        query: &topaz_kernel::HostQuery,
    ) -> topaz_kernel::HostFact {
        let path = topaz_resolve::normalize_path(query.logical_path());
        match query {
            topaz_kernel::HostQuery::ReadSource { .. } => {
                if let Some(source) = self.overlays.get(&path) {
                    topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Present(
                        source.clone(),
                    ))
                } else if let Some(package) = &self.package {
                    package.respond(request, query)
                } else {
                    topaz_kernel::HostFact::Source(topaz_kernel::SourceFact::Missing)
                }
            }
            topaz_kernel::HostQuery::ListDirectory { .. } => {
                if let Some(package) = &self.package {
                    return topaz_kernel::HostFact::Directory(
                        read_overlay_directory(&package.provider, &self.overlays, &path).into(),
                    );
                }
                let entries = overlay_directory_entries(&self.overlays, &path);
                let read = if entries.is_empty() {
                    topaz_resolve::DirectoryRead::Missing
                } else {
                    topaz_resolve::DirectoryRead::Present(entries)
                };
                topaz_kernel::HostFact::Directory(read.into())
            }
            topaz_kernel::HostQuery::PhysicalContainment { .. } => {
                if self.overlays.contains_key(&path) {
                    topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Inside {
                        alias_class: format!("lsp-overlay:{path}"),
                    })
                } else if let Some(package) = &self.package {
                    package.respond(request, query)
                } else {
                    topaz_kernel::HostFact::Containment(topaz_kernel::ContainmentFact::Missing)
                }
            }
        }
    }
}

pub(super) fn compile_self_lsp_snapshot(
    uri: &str,
    text: &str,
    version: LangVersion,
    workspace: Option<&LspWorkspace>,
    documents: &BTreeMap<String, String>,
    check_profile: Option<profile::CheckProfile>,
) -> Result<SelfLspSnapshot, String> {
    let (entry, language, package, facts) = if let Some(workspace) = workspace
        && let Some(entry) = lsp_workspace_relative_uri(workspace, uri)
    {
        let mut overlays = BTreeMap::new();
        for (document_uri, document_text) in documents {
            if let Some(path) = lsp_workspace_relative_uri(workspace, document_uri) {
                overlays.insert(path, document_text.clone());
            }
        }
        overlays.insert(entry.clone(), text.to_string());
        (
            entry,
            workspace.target.version,
            package_kernel_facts(&workspace.target),
            LspSelfFactHost {
                package: Some(PackageFactHost::new(&workspace.target)),
                overlays,
            },
        )
    } else {
        let entry = "main.tpz".to_string();
        let mut overlays = BTreeMap::new();
        overlays.insert(entry.clone(), text.to_string());
        (
            entry,
            version,
            topaz_kernel::PackageFacts::standalone(),
            LspSelfFactHost {
                package: None,
                overlays,
            },
        )
    };
    let request = topaz_kernel::KernelRequest::checked(&entry, Some(""), language, package)
        .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
    let typed = match check_profile {
        None => topaz_self_frontend::preview_linked_stage2_typed(&facts, request),
        Some(profile::CheckProfile::AgentPack) => {
            topaz_self_frontend::preview_linked_stage2_profiled_typed(
                &facts,
                request,
                topaz_self_frontend::CompilationProfile::AgentPack,
            )
        }
        Some(profile::CheckProfile::Bootstrap) => Err(
            "check profile `bootstrap` applies to a locked package, not an LSP source session"
                .to_string(),
        ),
        Some(profile::CheckProfile::TestProfile) => {
            Err("LSP check profile must be `agent-pack`".to_string())
        }
    }
    .map_err(|error| format!("self LSP compiler stopped: {error}"))?;
    if typed
        .resolved
        .modules
        .iter()
        .all(|module| module.path != entry)
    {
        return Err(format!(
            "self LSP product omitted active document `{entry}`"
        ));
    }
    Ok(SelfLspSnapshot { typed })
}

pub(super) fn self_lsp_module<'a>(
    snapshot: &'a SelfLspSnapshot,
    text: &str,
) -> Option<(usize, &'a topaz_kernel::CanonicalPreviewModule)> {
    snapshot
        .typed
        .resolved
        .modules
        .iter()
        .enumerate()
        .find(|(_, module)| module.source == text)
        .or_else(|| {
            snapshot
                .typed
                .resolved
                .modules
                .iter()
                .enumerate()
                .find(|(_, module)| module.entry)
        })
}

pub(super) fn self_lsp_span(lo: u32, hi: u32) -> Span {
    Span::new(FileId(0), lo, hi)
}

pub(super) fn self_lsp_declaration_at(
    snapshot: &SelfLspSnapshot,
    module_index: usize,
    offset: u32,
) -> Option<&topaz_kernel::CanonicalPreviewResolvedDeclaration> {
    snapshot
        .typed
        .resolved
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.module_index == module_index
                && declaration.lo <= offset
                && offset < declaration.hi
        })
        .min_by_key(|declaration| declaration.hi - declaration.lo)
}

pub(super) fn self_lsp_reference_at(
    snapshot: &SelfLspSnapshot,
    module_index: usize,
    offset: u32,
) -> Option<&topaz_kernel::CanonicalPreviewResolvedReference> {
    snapshot
        .typed
        .resolved
        .references
        .iter()
        .filter(|reference| {
            reference.module_index == module_index
                && reference.lo <= offset
                && offset < reference.hi
        })
        .min_by_key(|reference| reference.hi - reference.lo)
}

pub(super) fn self_lsp_target(
    snapshot: &SelfLspSnapshot,
    module_index: usize,
    offset: u32,
) -> Option<(usize, u32, u32)> {
    if let Some(reference) = self_lsp_reference_at(snapshot, module_index, offset) {
        return Some((
            reference.target_module_index?,
            reference.target_lo,
            reference.target_hi,
        ));
    }
    self_lsp_declaration_at(snapshot, module_index, offset)
        .map(|declaration| (declaration.module_index, declaration.lo, declaration.hi))
}

pub(super) struct LspOverlayProvider<'a> {
    pub(super) package: PackageProvider<'a>,
    pub(super) overlays: BTreeMap<String, String>,
}

pub(super) fn overlay_directory_entries(
    overlays: &BTreeMap<String, String>,
    dir: &str,
) -> Vec<(String, bool)> {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let mut entries = Vec::new();
    for path in overlays.keys() {
        let Some(rest) = path.strip_prefix(&prefix) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        match rest.split_once('/') {
            Some((head, _)) => entries.push((head.to_string(), true)),
            None => entries.push((rest.to_string(), false)),
        }
    }
    entries.sort();
    entries.dedup();
    entries
}

pub(super) fn read_overlay_directory(
    package: &PackageProvider<'_>,
    overlays: &BTreeMap<String, String>,
    dir: &str,
) -> topaz_resolve::DirectoryRead {
    let dir = topaz_resolve::normalize_path(dir);
    let overlay_entries = overlay_directory_entries(overlays, &dir);
    let mut entries = match package.read_directory(&dir) {
        topaz_resolve::DirectoryRead::Present(entries) => entries,
        topaz_resolve::DirectoryRead::Missing if overlay_entries.is_empty() => {
            return topaz_resolve::DirectoryRead::Missing;
        }
        topaz_resolve::DirectoryRead::Missing => Vec::new(),
        unreadable @ topaz_resolve::DirectoryRead::Unreadable { .. } => return unreadable,
    };
    entries.extend(overlay_entries);
    entries.sort();
    entries.dedup();
    topaz_resolve::DirectoryRead::Present(entries)
}

impl FileProvider for LspOverlayProvider<'_> {
    fn read(&self, path: &str) -> topaz_resolve::SourceRead {
        let path = topaz_resolve::normalize_path(path);
        self.overlays.get(&path).cloned().map_or_else(
            || self.package.read(&path),
            topaz_resolve::SourceRead::Present,
        )
    }

    fn is_extern_file(&self, path: &str) -> bool {
        self.package.is_extern_file(path)
    }

    fn is_extern_namespace(&self, identity: &str) -> bool {
        self.package.is_extern_namespace(identity)
    }

    fn extern_replay_error(&self, identity: &str) -> Option<String> {
        self.package.extern_replay_error(identity)
    }

    fn read_directory(&self, dir: &str) -> topaz_resolve::DirectoryRead {
        read_overlay_directory(&self.package, &self.overlays, dir)
    }

    fn physical_id(&self, path: &str) -> Option<String> {
        let path = topaz_resolve::normalize_path(path);
        self.package.physical_id(&path).or_else(|| {
            self.overlays
                .contains_key(&path)
                .then(|| format!("lsp-overlay:{path}"))
        })
    }
}

pub(super) fn self_lsp_diagnostics(
    snapshot: &SelfLspSnapshot,
) -> Result<(SourceMap, Vec<Diagnostic>), String> {
    let modules = &snapshot.typed.resolved.modules;
    let map = self_preview_source_map(modules)?;
    let mut diagnostics = snapshot
        .typed
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| self_resolver_diagnostic(diagnostic, modules))
        .collect::<Result<Vec<_>, _>>()?;
    diagnostics.extend(
        snapshot
            .typed
            .diagnostics
            .iter()
            .map(|diagnostic| self_checker_diagnostic(diagnostic, modules))
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok((map, diagnostics))
}

pub(super) fn self_lsp_publish_diagnostics_message(
    uri: &str,
    text: &str,
    snapshot: &SelfLspSnapshot,
) -> Result<String, String> {
    self_lsp_module(snapshot, text).ok_or_else(|| "self LSP has no active module".to_string())?;
    let mut out = String::from(
        "{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":",
    );
    push_json_string(&mut out, uri);
    out.push_str(",\"diagnostics\":[");
    let mut first = true;
    for diagnostic in snapshot.typed.resolved.diagnostics.iter() {
        if !first {
            out.push(',');
        }
        first = false;
        let source = snapshot
            .typed
            .resolved
            .modules
            .get(diagnostic.module_index)
            .map(|module| module.source.as_str())
            .unwrap_or(text);
        push_self_lsp_diagnostic(
            &mut out,
            source,
            &diagnostic.code,
            &diagnostic.message,
            diagnostic.lo,
            diagnostic.hi,
            None,
        );
    }
    for diagnostic in snapshot.typed.diagnostics.iter() {
        if !first {
            out.push(',');
        }
        first = false;
        let source = snapshot
            .typed
            .resolved
            .modules
            .get(diagnostic.module_index)
            .map(|module| module.source.as_str())
            .unwrap_or(text);
        push_self_lsp_diagnostic(
            &mut out,
            source,
            &diagnostic.code,
            &diagnostic.message,
            diagnostic.lo,
            diagnostic.hi,
            diagnostic.profile_rule.as_deref(),
        );
    }
    out.push_str("]}}");
    Ok(out)
}

pub(super) fn push_self_lsp_diagnostic(
    out: &mut String,
    text: &str,
    code: &str,
    message: &str,
    lo: u32,
    hi: u32,
    profile_rule: Option<&str>,
) {
    let (start_line, start_char) = lsp_position(text, lo);
    let (end_line, end_char) = lsp_position(text, hi);
    out.push_str("{\"range\":{\"start\":{\"line\":");
    let _ = write!(
        out,
        "{start_line},\"character\":{start_char}}},\"end\":{{\"line\":{end_line},\"character\":{end_char}}}}}"
    );
    out.push_str(",\"severity\":1,\"code\":");
    push_json_string(out, code);
    out.push_str(",\"source\":\"topaz\",\"message\":");
    push_json_string(out, message);
    if let Some(profile_rule) = profile_rule {
        out.push_str(",\"data\":{\"profileRule\":");
        push_json_string(out, profile_rule);
        out.push('}');
    }
    out.push('}');
}

pub(super) fn self_lsp_hover_message(
    id: &str,
    text: &str,
    line: u32,
    character: u32,
    snapshot: &SelfLspSnapshot,
) -> String {
    let offset = lsp_offset(text, line, character);
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":");
    let Some((module_index, module)) = self_lsp_module(snapshot, text) else {
        out.push_str("null}");
        return out;
    };
    let target = self_lsp_target(snapshot, module_index, offset);
    let typed =
        target
            .and_then(|(target_module, lo, hi)| {
                let identity = &snapshot.typed.resolved.modules.get(target_module)?.identity;
                snapshot.typed.nodes.iter().find(|node| {
                    node.module == *identity && node.span.lo == lo && node.span.hi == hi
                })
            })
            .or_else(|| {
                snapshot
                    .typed
                    .nodes
                    .iter()
                    .filter(|node| {
                        node.module == module.identity
                            && node.span.lo <= offset
                            && offset < node.span.hi
                    })
                    .min_by_key(|node| node.span.hi - node.span.lo)
            });
    let Some(typed) = typed else {
        out.push_str("null}");
        return out;
    };
    let span = self_lsp_reference_at(snapshot, module_index, offset)
        .map(|reference| self_lsp_span(reference.lo, reference.hi))
        .or_else(|| {
            self_lsp_declaration_at(snapshot, module_index, offset)
                .map(|declaration| self_lsp_span(declaration.lo, declaration.hi))
        })
        .unwrap_or_else(|| self_lsp_span(typed.span.lo, typed.span.hi));
    let name = span_text(text, span);
    let value = format!("```topaz\n{name}: {}\n```", render_semantic_type(&typed.ty));
    out.push_str("{\"contents\":{\"kind\":\"markdown\",\"value\":");
    push_json_string(&mut out, &value);
    out.push_str("},\"range\":");
    push_lsp_range(&mut out, text, span);
    out.push_str("}}");
    out
}

pub(super) fn self_lsp_definition_message(
    id: &str,
    uri: &str,
    text: &str,
    line: u32,
    character: u32,
    snapshot: &SelfLspSnapshot,
) -> String {
    let offset = lsp_offset(text, line, character);
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":");
    let Some((module_index, _)) = self_lsp_module(snapshot, text) else {
        out.push_str("null}");
        return out;
    };
    let Some((target_module, lo, hi)) = self_lsp_target(snapshot, module_index, offset) else {
        out.push_str("null}");
        return out;
    };
    if target_module != module_index {
        out.push_str("null}");
        return out;
    }
    push_lsp_location_result(&mut out, uri, text, self_lsp_span(lo, hi));
    out.push('}');
    out
}

pub(super) fn self_lsp_reference_spans(
    snapshot: &SelfLspSnapshot,
    module_index: usize,
    offset: u32,
    include_declaration: bool,
) -> Vec<Span> {
    let Some((target_module, target_lo, target_hi)) =
        self_lsp_target(snapshot, module_index, offset)
    else {
        return Vec::new();
    };
    if target_module != module_index {
        return Vec::new();
    }
    let mut spans = snapshot
        .typed
        .resolved
        .references
        .iter()
        .filter(|reference| {
            reference.module_index == module_index
                && reference.target_module_index == Some(target_module)
                && reference.target_lo == target_lo
                && reference.target_hi == target_hi
        })
        .map(|reference| self_lsp_span(reference.lo, reference.hi))
        .collect::<Vec<_>>();
    if include_declaration {
        spans.push(self_lsp_span(target_lo, target_hi));
    }
    spans.sort_by_key(|span| (span.lo, span.hi));
    spans.dedup_by_key(|span| (span.lo, span.hi));
    spans
}

pub(super) fn self_lsp_references_message(
    id: &str,
    uri: &str,
    text: &str,
    line: u32,
    character: u32,
    include_declaration: bool,
    snapshot: &SelfLspSnapshot,
) -> String {
    let spans = self_lsp_module(snapshot, text)
        .map(|(module_index, _)| {
            self_lsp_reference_spans(
                snapshot,
                module_index,
                lsp_offset(text, line, character),
                include_declaration,
            )
        })
        .unwrap_or_default();
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":[");
    for (index, span) in spans.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_lsp_location(&mut out, uri, text, *span);
    }
    out.push_str("]}");
    out
}

pub(super) fn self_lsp_identifier_is_valid(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if name == "_" || !(first == '_' || first.is_alphabetic() || !first.is_ascii()) {
        return false;
    }
    if !chars.all(|ch| ch == '_' || ch.is_alphanumeric() || !ch.is_ascii()) {
        return false;
    }
    !TOPAZ_COMPLETION_KEYWORDS.contains(&name)
}

pub(super) fn self_lsp_rename_message(
    id: &str,
    uri: &str,
    text: &str,
    line: u32,
    character: u32,
    new_name: &str,
    snapshot: &SelfLspSnapshot,
) -> String {
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":");
    if !self_lsp_identifier_is_valid(new_name) {
        out.push_str("null}");
        return out;
    }
    let spans = self_lsp_module(snapshot, text)
        .map(|(module_index, _)| {
            self_lsp_reference_spans(
                snapshot,
                module_index,
                lsp_offset(text, line, character),
                true,
            )
        })
        .unwrap_or_default();
    if spans.is_empty() {
        out.push_str("null}");
        return out;
    }
    out.push_str("{\"changes\":{");
    push_json_string(&mut out, uri);
    out.push_str(":[");
    for (index, span) in spans.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_lsp_text_edit(&mut out, text, *span, new_name);
    }
    out.push_str("]}}}");
    out
}

pub(super) fn self_lsp_symbol_for_node(
    module: &topaz_kernel::CanonicalPreviewModule,
    index: usize,
) -> Option<LspDocumentSymbol> {
    let node = module.ast.get(index)?;
    if node.kind == "statement/export" {
        let child = module
            .ast
            .iter()
            .enumerate()
            .find(|(_, child)| child.parent == Some(index as u32))?;
        return self_lsp_symbol_for_node(module, child.0);
    }
    let name = self_ast_children(module, index, "name")
        .first()
        .map(|(_, node)| *node)
        .or_else(|| {
            module.ast.iter().enumerate().find_map(|(candidate, node)| {
                if node.kind != "identifier" || node.field != "name" {
                    return None;
                }
                let mut parent = node.parent.map(|value| value as usize);
                while let Some(parent_index) = parent {
                    if parent_index == index {
                        return Some(node);
                    }
                    parent = module
                        .ast
                        .get(parent_index)?
                        .parent
                        .map(|value| value as usize);
                }
                let _ = candidate;
                None
            })
        })?;
    let name_text = self_ast_text(module, name).ok()?;
    let (kind, child_field, child_kind) = match node.kind.as_str() {
        "statement/function" | "function-declaration" => (12, "", 0),
        "statement/type-alias" | "statement/newtype" => (5, "", 0),
        "statement/enum" => (10, "variants", 22),
        "statement/record" => (23, "fields", 8),
        "statement/protocol" => (11, "methods", 6),
        "statement/let" => (13, "", 0),
        "statement/const" => (14, "", 0),
        _ => return None,
    };
    let children = if child_field.is_empty() {
        Vec::new()
    } else {
        self_ast_children(module, index, child_field)
            .into_iter()
            .filter_map(|(child_index, child)| {
                let name = self_ast_children(module, child_index, "name")
                    .first()
                    .map(|(_, node)| *node)?;
                Some(lsp_symbol(
                    self_ast_text(module, name).ok()?,
                    child_kind,
                    self_lsp_span(child.lo, child.hi),
                    self_lsp_span(name.lo, name.hi),
                    Vec::new(),
                ))
            })
            .collect()
    };
    Some(lsp_symbol(
        name_text,
        kind,
        self_lsp_span(node.lo, node.hi),
        self_lsp_span(name.lo, name.hi),
        children,
    ))
}

pub(super) fn self_lsp_document_symbols(
    snapshot: &SelfLspSnapshot,
    text: &str,
) -> Vec<LspDocumentSymbol> {
    let Some((_, module)) = self_lsp_module(snapshot, text) else {
        return Vec::new();
    };
    module
        .ast
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            node.parent.is_some_and(|parent| {
                module
                    .ast
                    .get(parent as usize)
                    .is_some_and(|parent| parent.kind == "program")
            })
        })
        .filter_map(|(index, _)| self_lsp_symbol_for_node(module, index))
        .collect()
}

pub(super) fn self_lsp_document_symbol_message(
    id: &str,
    text: &str,
    snapshot: &SelfLspSnapshot,
) -> String {
    let symbols = self_lsp_document_symbols(snapshot, text);
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":[");
    for (index, symbol) in symbols.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_lsp_document_symbol(&mut out, text, symbol);
    }
    out.push_str("]}");
    out
}

pub(super) fn self_lsp_completion_message(
    id: &str,
    text: &str,
    line: u32,
    character: u32,
    snapshot: &SelfLspSnapshot,
) -> String {
    let mut items = BTreeMap::new();
    let receiver = lsp_member_completion_receiver(text, line, character);
    let mut member_completed = false;
    if let Some(receiver) = &receiver
        && let Some((module_index, _)) = self_lsp_module(snapshot, text)
    {
        if let Some((target_module, lo, hi)) =
            self_lsp_target(snapshot, module_index, receiver.span.lo)
        {
            member_completed = true;
            if let Some(namespace) = snapshot
                .typed
                .resolved
                .declarations
                .iter()
                .find(|declaration| {
                    declaration.module_index == target_module
                        && declaration.lo == lo
                        && declaration.hi == hi
                        && declaration.declaration_kind == "namespace-import"
                })
                .and_then(|declaration| declaration.target_module.as_deref())
                && let Some(target_index) = snapshot
                    .typed
                    .resolved
                    .modules
                    .iter()
                    .position(|candidate| candidate.identity == namespace)
            {
                for export in snapshot
                    .typed
                    .resolved
                    .exports
                    .iter()
                    .filter(|export| export.module_index == target_index)
                {
                    insert_completion(
                        &mut items,
                        &export.name,
                        if export.namespace == "type" { 7 } else { 3 },
                    );
                }
            }
            if let Some(identity) = snapshot
                .typed
                .resolved
                .modules
                .get(target_module)
                .map(|module| &module.identity)
                && let Some(node) = snapshot.typed.nodes.iter().find(|node| {
                    node.module == *identity && node.span.lo == lo && node.span.hi == hi
                })
                && let Ok(ty) = self_semantic_type(&node.ty)
            {
                for member in topaz_check::builtins::receiver_member_names(&ty) {
                    let kind = match topaz_check::builtins::receiver_member(&ty, member) {
                        Some(topaz_check::builtins::Member::Method(_)) => 2,
                        Some(topaz_check::builtins::Member::Property(_)) => 10,
                        None => continue,
                    };
                    insert_completion(&mut items, member, kind);
                }
            }
        } else {
            member_completed = lsp_insert_static_member_completions(&mut items, &receiver.name);
        }
    }
    if !member_completed {
        if lsp_is_import_completion(text, line, character) {
            for module in topaz_resolve::std_module_identities() {
                insert_completion(&mut items, module, 9);
            }
        } else {
            for symbol in self_lsp_document_symbols(snapshot, text) {
                insert_completion_symbol(&mut items, &symbol);
            }
            if let Some((module_index, _)) = self_lsp_module(snapshot, text) {
                for declaration in snapshot
                    .typed
                    .resolved
                    .declarations
                    .iter()
                    .filter(|declaration| declaration.module_index == module_index)
                {
                    insert_completion(&mut items, &declaration.name, 6);
                }
            }
            for keyword in TOPAZ_COMPLETION_KEYWORDS {
                insert_completion(&mut items, keyword, 14);
            }
            lsp_insert_global_builtin_completions(&mut items);
            for module in topaz_resolve::std_module_identities() {
                insert_completion(&mut items, module, 9);
            }
        }
    }
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":[");
    for (index, (label, kind)) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"label\":");
        push_json_string(&mut out, label);
        let _ = write!(out, ",\"kind\":{kind}}}");
    }
    out.push_str("]}");
    out
}

pub(super) fn self_lsp_signature_from_declaration(
    snapshot: &SelfLspSnapshot,
    target_module: usize,
    lo: u32,
    hi: u32,
    display_name: &str,
) -> Option<LspSignature> {
    let module = snapshot.typed.resolved.modules.get(target_module)?;
    let declaration = self_ast_declaration_index(module, lo, hi)?;
    let node = module.ast.get(declaration)?;
    let function = if node.kind == "statement/function" {
        self_ast_children(module, declaration, "declaration")
            .first()
            .map(|(index, _)| *index)?
    } else if node.kind == "function-declaration" {
        declaration
    } else {
        return None;
    };
    let mut parameters = Vec::new();
    for (parameter_index, parameter) in self_ast_children(module, function, "parameters") {
        let name = self_ast_named_child(module, parameter_index, "name").ok()?;
        let ty = self_ast_named_child(module, parameter_index, "type").ok()?;
        let variadic = parameter.attributes.iter().any(|attribute| {
            attribute.name == "variadic"
                && attribute.value == topaz_kernel::CanonicalPreviewAstValue::Bool(true)
        });
        parameters.push(format!(
            "{}{}: {}",
            if variadic { "..." } else { "" },
            self_ast_text(module, name).ok()?,
            self_ast_text(module, ty).ok()?
        ));
    }
    let result = self_ast_children(module, function, "returnType")
        .first()
        .and_then(|(_, node)| self_ast_text(module, node).ok())
        .unwrap_or("()");
    Some(LspSignature {
        label: format!("{display_name}({}) -> {result}", parameters.join(", ")),
        parameters,
    })
}

pub(super) fn self_lsp_signature_help_message(
    id: &str,
    text: &str,
    line: u32,
    character: u32,
    snapshot: &SelfLspSnapshot,
) -> String {
    let offset = lsp_offset(text, line, character);
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":");
    let Some(call) = lsp_call_name_before(text, offset) else {
        out.push_str("null}");
        return out;
    };
    let target = self_lsp_module(snapshot, text)
        .and_then(|(module_index, _)| self_lsp_target(snapshot, module_index, call.span.lo));
    let signature = match target {
        Some((target_module, lo, hi)) => {
            self_lsp_signature_from_declaration(snapshot, target_module, lo, hi, &call.name)
        }
        None => lsp_signature_for_builtin(&call.name),
    };
    let Some(signature) = signature else {
        out.push_str("null}");
        return out;
    };
    let active_parameter = if signature.parameters.is_empty() {
        0
    } else {
        call.active_parameter
            .min((signature.parameters.len() - 1) as u32)
    };
    out.push_str("{\"signatures\":[{\"label\":");
    push_json_string(&mut out, &signature.label);
    out.push_str(",\"parameters\":[");
    for (index, parameter) in signature.parameters.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"label\":");
        push_json_string(&mut out, parameter);
        out.push('}');
    }
    out.push_str("]}],\"activeSignature\":0,\"activeParameter\":");
    let _ = write!(out, "{active_parameter}");
    out.push_str("}}");
    out
}

pub(super) fn self_lsp_code_action_message(
    id: &str,
    uri: &str,
    text: &str,
    snapshot: &SelfLspSnapshot,
) -> Result<String, String> {
    let (_, diagnostics) = self_lsp_diagnostics(snapshot)?;
    let Some((module_index, _)) = self_lsp_module(snapshot, text) else {
        return Ok(format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":[]}}"));
    };
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":[");
    let mut first = true;
    for diagnostic in diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.primary.span.file.0 as usize == module_index)
    {
        let Some(replacement) = lsp_diagnostic_replacement(diagnostic) else {
            continue;
        };
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str("{\"title\":");
        push_json_string(&mut out, &format!("Replace with `{replacement}`"));
        out.push_str(",\"kind\":\"quickfix\",\"edit\":{\"changes\":{");
        push_json_string(&mut out, uri);
        out.push_str(":[");
        push_lsp_text_edit(&mut out, text, diagnostic.primary.span, &replacement);
        out.push_str("]}}}");
    }
    out.push_str("]}");
    Ok(out)
}

pub(super) fn lsp_initialize_check_profile(
    message: &str,
) -> Result<Option<profile::CheckProfile>, String> {
    use topaz_value::JsonValue;

    let parsed = topaz_value::json_parse(message)
        .map_err(|error| format!("initialize request is not valid JSON: {error:?}"))?;
    let JsonValue::Object(root) = &parsed else {
        return Err("initialize request must be a JSON object".to_string());
    };
    let Some(JsonValue::Object(params)) = root.get("params") else {
        return Ok(None);
    };
    let Some(JsonValue::Object(initialization_options)) = params.get("initializationOptions")
    else {
        return Ok(None);
    };
    let Some(JsonValue::Object(topaz)) = initialization_options.get("topaz") else {
        return Ok(None);
    };
    let Some(value) = topaz.get("checkProfile") else {
        return Ok(None);
    };
    let JsonValue::String(value) = value else {
        return Err(
            "params.initializationOptions.topaz.checkProfile must be the string `agent-pack`"
                .to_string(),
        );
    };
    match value.as_ref() {
        "agent-pack" => Ok(Some(profile::CheckProfile::AgentPack)),
        "bootstrap" => Err(
            "check profile `bootstrap` applies to a locked package, not a standalone LSP source"
                .to_string(),
        ),
        value => Err(format!(
            "params.initializationOptions.topaz.checkProfile must be `agent-pack`; received `{value}`"
        )),
    }
}
