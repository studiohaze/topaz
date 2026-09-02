use crate::*;

pub(super) fn lsp_stdio(
    version: LangVersion,
    root: Option<&str>,
    compiler: CompilerSelection,
) -> ExitCode {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = std::io::BufReader::new(stdin.lock());
    let mut output = stdout.lock();
    let mut documents: BTreeMap<String, String> = BTreeMap::new();
    let mut workspace = root.and_then(|root| lsp_workspace(Path::new(root)));
    let mut check_profile = None;
    let mut self_session = if compiler == CompilerSelection::SelfHosted {
        match SelfLspSession::new() {
            Ok(session) => Some(session),
            Err(error) => {
                eprintln!("topaz lsp: {error}");
                eprintln!("topaz: recovery: rerun `topaz lsp --compiler rust` (not executed)");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };

    loop {
        let message = match read_lsp_message(&mut input) {
            Ok(Some(message)) => message,
            Ok(None) => return ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("topaz lsp: {e}");
                return ExitCode::FAILURE;
            }
        };
        let method = json_string_field(&message, "method");
        let id = json_id_raw(&message);
        let write = |output: &mut std::io::StdoutLock<'_>, body: String| -> Result<(), String> {
            write_lsp_message(output, &body).map_err(|e| format!("cannot write response: {e}"))
        };

        let result = match method.as_deref() {
            Some("initialize") => match lsp_initialize_check_profile(&message) {
                Err(error) => {
                    if let Some(id) = id {
                        let mut body = format!(
                            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":-32602,\"message\":"
                        );
                        push_json_string(&mut body, &error);
                        body.push_str("}}");
                        write(&mut output, body)
                    } else {
                        Ok(())
                    }
                }
                Ok(selected_profile) => {
                    check_profile = selected_profile;
                    if let Some(session) = self_session.as_mut() {
                        session.set_check_profile(selected_profile);
                    }
                    if let Some(root_uri) = json_string_field(&message, "rootUri")
                        && let Some(root) = lsp_file_uri_path(&root_uri)
                    {
                        workspace = lsp_workspace(&root);
                        if let Some(session) = self_session.as_mut() {
                            session.invalidate();
                        }
                    }
                    if let Some(id) = id {
                        let body = format!(
                            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"capabilities\":{{\"textDocumentSync\":1,\"hoverProvider\":true,\"definitionProvider\":true,\"referencesProvider\":true,\"renameProvider\":true,\"documentSymbolProvider\":true,\"completionProvider\":{{\"triggerCharacters\":[\".\",\" \",\":\"]}},\"signatureHelpProvider\":{{\"triggerCharacters\":[\"(\",\",\"]}},\"codeActionProvider\":{{\"codeActionKinds\":[\"quickfix\"]}}}}}}}}"
                        );
                        write(&mut output, body)
                    } else {
                        Ok(())
                    }
                }
            },
            Some("initialized") => Ok(()),
            Some("textDocument/didOpen") => {
                if let (Some(uri), Some(text)) = (
                    json_string_field(&message, "uri"),
                    json_string_field(&message, "text"),
                ) {
                    documents.insert(uri.clone(), text.clone());
                    if let Some(session) = self_session.as_mut() {
                        session.invalidate();
                        match session.snapshot(&uri, &text, version, workspace.as_ref(), &documents)
                        {
                            Ok(snapshot) => {
                                self_lsp_publish_diagnostics_message(&uri, &text, snapshot)
                                    .and_then(|body| write(&mut output, body))
                            }
                            Err(error) => Err(error),
                        }
                    } else {
                        lsp_publish_diagnostics_message(
                            &uri,
                            &text,
                            version,
                            workspace.as_ref(),
                            &documents,
                            check_profile,
                        )
                        .and_then(|body| write(&mut output, body))
                    }
                } else {
                    Ok(())
                }
            }
            Some("textDocument/didChange") => {
                if let Some(uri) = json_string_field(&message, "uri") {
                    if let Some(text) = json_string_field(&message, "text") {
                        documents.insert(uri.clone(), text.clone());
                    }
                    let text = documents.get(&uri).cloned().unwrap_or_default();
                    if let Some(session) = self_session.as_mut() {
                        session.invalidate();
                        match session.snapshot(&uri, &text, version, workspace.as_ref(), &documents)
                        {
                            Ok(snapshot) => {
                                self_lsp_publish_diagnostics_message(&uri, &text, snapshot)
                                    .and_then(|body| write(&mut output, body))
                            }
                            Err(error) => Err(error),
                        }
                    } else {
                        lsp_publish_diagnostics_message(
                            &uri,
                            &text,
                            version,
                            workspace.as_ref(),
                            &documents,
                            check_profile,
                        )
                        .and_then(|body| write(&mut output, body))
                    }
                } else {
                    Ok(())
                }
            }
            Some("textDocument/didClose") => {
                if let Some(uri) = json_string_field(&message, "uri") {
                    documents.remove(&uri);
                    if let Some(session) = self_session.as_mut() {
                        session.invalidate();
                    }
                    write(&mut output, lsp_empty_diagnostics_message(&uri))
                } else {
                    Ok(())
                }
            }
            Some("textDocument/hover") => {
                if let (Some(id), Some(uri), Some(line), Some(character)) = (
                    id,
                    json_string_field(&message, "uri"),
                    json_u32_field(&message, "line"),
                    json_u32_field(&message, "character"),
                ) {
                    let text = documents.get(&uri).cloned().unwrap_or_default();
                    if let Some(session) = self_session.as_mut() {
                        match session.snapshot(&uri, &text, version, workspace.as_ref(), &documents)
                        {
                            Ok(snapshot) => write(
                                &mut output,
                                self_lsp_hover_message(&id, &text, line, character, snapshot),
                            ),
                            Err(error) => Err(error),
                        }
                    } else {
                        write(
                            &mut output,
                            lsp_hover_message(&id, &text, line, character, version),
                        )
                    }
                } else {
                    Ok(())
                }
            }
            Some("textDocument/definition") => {
                if let (Some(id), Some(uri), Some(line), Some(character)) = (
                    id,
                    json_string_field(&message, "uri"),
                    json_u32_field(&message, "line"),
                    json_u32_field(&message, "character"),
                ) {
                    let text = documents.get(&uri).cloned().unwrap_or_default();
                    if let Some(session) = self_session.as_mut() {
                        match session.snapshot(&uri, &text, version, workspace.as_ref(), &documents)
                        {
                            Ok(snapshot) => write(
                                &mut output,
                                self_lsp_definition_message(
                                    &id, &uri, &text, line, character, snapshot,
                                ),
                            ),
                            Err(error) => Err(error),
                        }
                    } else {
                        write(
                            &mut output,
                            lsp_definition_message(&id, &uri, &text, line, character, version),
                        )
                    }
                } else {
                    Ok(())
                }
            }
            Some("textDocument/references") => {
                if let (Some(id), Some(uri), Some(line), Some(character)) = (
                    id,
                    json_string_field(&message, "uri"),
                    json_u32_field(&message, "line"),
                    json_u32_field(&message, "character"),
                ) {
                    let text = documents.get(&uri).cloned().unwrap_or_default();
                    let include_declaration =
                        json_bool_field(&message, "includeDeclaration").unwrap_or(true);
                    if let Some(session) = self_session.as_mut() {
                        match session.snapshot(&uri, &text, version, workspace.as_ref(), &documents)
                        {
                            Ok(snapshot) => write(
                                &mut output,
                                self_lsp_references_message(
                                    &id,
                                    &uri,
                                    &text,
                                    line,
                                    character,
                                    include_declaration,
                                    snapshot,
                                ),
                            ),
                            Err(error) => Err(error),
                        }
                    } else {
                        write(
                            &mut output,
                            lsp_references_message(
                                &id,
                                &uri,
                                &text,
                                line,
                                character,
                                version,
                                include_declaration,
                            ),
                        )
                    }
                } else {
                    Ok(())
                }
            }
            Some("textDocument/completion") => {
                if let (Some(id), Some(uri), Some(line), Some(character)) = (
                    id,
                    json_string_field(&message, "uri"),
                    json_u32_field(&message, "line"),
                    json_u32_field(&message, "character"),
                ) {
                    let text = documents.get(&uri).cloned().unwrap_or_default();
                    if let Some(session) = self_session.as_mut() {
                        match session.snapshot(&uri, &text, version, workspace.as_ref(), &documents)
                        {
                            Ok(snapshot) => write(
                                &mut output,
                                self_lsp_completion_message(&id, &text, line, character, snapshot),
                            ),
                            Err(error) => Err(error),
                        }
                    } else {
                        write(
                            &mut output,
                            lsp_completion_message(&id, &text, line, character, version),
                        )
                    }
                } else {
                    Ok(())
                }
            }
            Some("textDocument/signatureHelp") => {
                if let (Some(id), Some(uri), Some(line), Some(character)) = (
                    id,
                    json_string_field(&message, "uri"),
                    json_u32_field(&message, "line"),
                    json_u32_field(&message, "character"),
                ) {
                    let text = documents.get(&uri).cloned().unwrap_or_default();
                    if let Some(session) = self_session.as_mut() {
                        match session.snapshot(&uri, &text, version, workspace.as_ref(), &documents)
                        {
                            Ok(snapshot) => write(
                                &mut output,
                                self_lsp_signature_help_message(
                                    &id, &text, line, character, snapshot,
                                ),
                            ),
                            Err(error) => Err(error),
                        }
                    } else {
                        write(
                            &mut output,
                            lsp_signature_help_message(&id, &text, line, character, version),
                        )
                    }
                } else {
                    Ok(())
                }
            }
            Some("textDocument/codeAction") => {
                if let (Some(id), Some(uri)) = (id, json_string_field(&message, "uri")) {
                    let text = documents.get(&uri).cloned().unwrap_or_default();
                    if let Some(session) = self_session.as_mut() {
                        match session.snapshot(&uri, &text, version, workspace.as_ref(), &documents)
                        {
                            Ok(snapshot) => {
                                self_lsp_code_action_message(&id, &uri, &text, snapshot)
                                    .and_then(|body| write(&mut output, body))
                            }
                            Err(error) => Err(error),
                        }
                    } else {
                        write(
                            &mut output,
                            lsp_code_action_message(&id, &uri, &text, version),
                        )
                    }
                } else {
                    Ok(())
                }
            }
            Some("textDocument/rename") => {
                if let (Some(id), Some(uri), Some(line), Some(character), Some(new_name)) = (
                    id,
                    json_string_field(&message, "uri"),
                    json_u32_field(&message, "line"),
                    json_u32_field(&message, "character"),
                    json_string_field(&message, "newName"),
                ) {
                    let text = documents.get(&uri).cloned().unwrap_or_default();
                    if let Some(session) = self_session.as_mut() {
                        match session.snapshot(&uri, &text, version, workspace.as_ref(), &documents)
                        {
                            Ok(snapshot) => write(
                                &mut output,
                                self_lsp_rename_message(
                                    &id, &uri, &text, line, character, &new_name, snapshot,
                                ),
                            ),
                            Err(error) => Err(error),
                        }
                    } else {
                        write(
                            &mut output,
                            lsp_rename_message(
                                &id, &uri, &text, line, character, version, &new_name,
                            ),
                        )
                    }
                } else {
                    Ok(())
                }
            }
            Some("textDocument/documentSymbol") => {
                if let (Some(id), Some(uri)) = (id, json_string_field(&message, "uri")) {
                    let text = documents.get(&uri).cloned().unwrap_or_default();
                    if let Some(session) = self_session.as_mut() {
                        match session.snapshot(&uri, &text, version, workspace.as_ref(), &documents)
                        {
                            Ok(snapshot) => write(
                                &mut output,
                                self_lsp_document_symbol_message(&id, &text, snapshot),
                            ),
                            Err(error) => Err(error),
                        }
                    } else {
                        write(
                            &mut output,
                            lsp_document_symbol_message(&id, &text, version),
                        )
                    }
                } else {
                    Ok(())
                }
            }
            Some("shutdown") => {
                if let Some(id) = id {
                    write(
                        &mut output,
                        format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":null}}"),
                    )
                } else {
                    Ok(())
                }
            }
            Some("exit") => return ExitCode::SUCCESS,
            Some(_) | None => {
                if let Some(id) = id {
                    write(
                        &mut output,
                        format!(
                            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":-32601,\"message\":\"method not found\"}}}}"
                        ),
                    )
                } else {
                    Ok(())
                }
            }
        };
        if let Err(e) = result {
            eprintln!("topaz lsp: {e}");
            return ExitCode::FAILURE;
        }
    }
}

pub(super) fn lsp_workspace(root: &Path) -> Option<LspWorkspace> {
    if !root.join("topaz.toml").is_file() {
        return None;
    }
    let logical_root = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(root)
    };
    package_target(root.to_str(), None, true)
        .ok()
        .map(|target| LspWorkspace {
            target,
            logical_root,
        })
}

pub(super) fn lsp_file_uri_path(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    let encoded = encoded.strip_prefix("localhost").unwrap_or(encoded);
    let mut bytes = Vec::with_capacity(encoded.len());
    let raw = encoded.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' && index + 2 < raw.len() {
            let high = hex_nibble(raw[index + 1])?;
            let low = hex_nibble(raw[index + 2])?;
            bytes.push((high << 4) | low);
            index += 3;
        } else {
            bytes.push(raw[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(bytes).ok()?;
    #[cfg(windows)]
    let decoded = decoded
        .strip_prefix('/')
        .filter(|rest| rest.as_bytes().get(1) == Some(&b':'))
        .unwrap_or(&decoded)
        .to_string();
    Some(PathBuf::from(decoded))
}

pub(super) fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(super) fn read_lsp_message<R: BufRead>(input: &mut R) -> Result<Option<String>, String> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let n = input
            .read_line(&mut line)
            .map_err(|e| format!("cannot read header: {e}"))?;
        if n == 0 {
            return if content_length.is_some() {
                Err("unexpected EOF in LSP headers".to_string())
            } else {
                Ok(None)
            };
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            let len = value
                .trim()
                .parse::<usize>()
                .map_err(|_| format!("invalid Content-Length `{}`", value.trim()))?;
            content_length = Some(len);
        }
    }
    let len = content_length.ok_or_else(|| "missing Content-Length".to_string())?;
    let mut body = vec![0; len];
    input
        .read_exact(&mut body)
        .map_err(|e| format!("cannot read body: {e}"))?;
    String::from_utf8(body).map(Some).map_err(|e| e.to_string())
}

pub(super) fn write_lsp_message<W: std::io::Write>(
    output: &mut W,
    body: &str,
) -> std::io::Result<()> {
    write!(output, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    output.flush()
}

pub(super) fn lsp_publish_diagnostics_message(
    uri: &str,
    text: &str,
    version: LangVersion,
    workspace: Option<&LspWorkspace>,
    documents: &BTreeMap<String, String>,
    check_profile: Option<profile::CheckProfile>,
) -> Result<String, String> {
    let (map, diagnostics) =
        lsp_document_diagnostics(uri, text, version, workspace, documents, check_profile)?;
    let mut out = String::from(
        "{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":",
    );
    push_json_string(&mut out, uri);
    out.push_str(",\"diagnostics\":[");
    for (i, diag) in diagnostics.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_lsp_diagnostic(&mut out, &map, diag);
    }
    out.push_str("]}}");
    Ok(out)
}

pub(super) fn lsp_empty_diagnostics_message(uri: &str) -> String {
    let mut out = String::from(
        "{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/publishDiagnostics\",\"params\":{\"uri\":",
    );
    push_json_string(&mut out, uri);
    out.push_str(",\"diagnostics\":[]}}");
    out
}

pub(super) fn lsp_hover_message(
    id: &str,
    text: &str,
    line: u32,
    character: u32,
    version: LangVersion,
) -> String {
    let offset = lsp_offset(text, line, character);
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":");
    let Some((name, ty, span)) = lsp_hover_type_at(text, offset, version) else {
        out.push_str("null}");
        return out;
    };
    out.push_str("{\"contents\":{\"kind\":\"markdown\",\"value\":");
    let mut value = String::from("```topaz\n");
    value.push_str(&name);
    value.push_str(": ");
    value.push_str(&ty);
    value.push_str("\n```");
    push_json_string(&mut out, &value);
    let (start_line, start_char) = lsp_position(text, span.lo);
    let (end_line, end_char) = lsp_position(text, span.hi);
    out.push_str("},\"range\":{\"start\":{\"line\":");
    let _ = write!(out, "{start_line},\"character\":{start_char}");
    out.push_str("},\"end\":{\"line\":");
    let _ = write!(out, "{end_line},\"character\":{end_char}");
    out.push_str("}}}}");
    out
}

pub(super) fn lsp_hover_type_at(
    text: &str,
    offset: u32,
    version: LangVersion,
) -> Option<(String, String, topaz_diag::Span)> {
    let checked = lsp_checked_unit(text, version)?;
    checked
        .hover_types
        .into_iter()
        .filter(|h| h.span.lo <= offset && offset < h.span.hi)
        .min_by_key(|h| h.span.hi - h.span.lo)
        .map(|h| (h.name, h.ty, h.span))
}

pub(super) fn lsp_checked_unit(
    text: &str,
    version: LangVersion,
) -> Option<topaz_check::CheckedUnit> {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", text);
    let out = resolve_with_version(&provider, "main.tpz", None, version);
    if has_errors(&out.diagnostics) {
        return None;
    }
    let unit = unit_modules(&out);
    let checked = topaz_check::check_unit_typed_with_version(&unit, version);
    if has_errors(&checked.diagnostics) {
        return None;
    }
    Some(checked)
}

pub(super) fn lsp_document_diagnostics(
    uri: &str,
    text: &str,
    version: LangVersion,
    workspace: Option<&LspWorkspace>,
    documents: &BTreeMap<String, String>,
    check_profile: Option<profile::CheckProfile>,
) -> Result<(SourceMap, Vec<LspPublishedDiagnostic>), String> {
    let Some(workspace) = workspace else {
        let mut provider = InMemoryProvider::new();
        provider.add_file("main.tpz", text);
        let out = resolve_with_version(&provider, "main.tpz", None, version);
        return lsp_checked_diagnostics(out, version, check_profile);
    };
    let Some(entry) = lsp_workspace_relative_uri(workspace, uri) else {
        let mut provider = InMemoryProvider::new();
        provider.add_file("main.tpz", text);
        let out = resolve_with_version(&provider, "main.tpz", None, version);
        return lsp_checked_diagnostics(out, version, check_profile);
    };
    let mut overlays = BTreeMap::new();
    for (document_uri, document_text) in documents {
        if let Some(path) = lsp_workspace_relative_uri(workspace, document_uri) {
            overlays.insert(path, document_text.clone());
        }
    }
    overlays.insert(entry.clone(), text.to_string());
    let provider = LspOverlayProvider {
        package: PackageProvider::new(&workspace.target),
        overlays,
    };
    let out = resolve_with_version(&provider, &entry, Some(""), workspace.target.version);
    lsp_checked_diagnostics(out, workspace.target.version, check_profile)
}

pub(super) fn lsp_workspace_relative_uri(workspace: &LspWorkspace, uri: &str) -> Option<String> {
    let path = lsp_file_uri_path(uri)?;
    let canonical = fs::canonicalize(&path).ok();
    let relative = logical_root_relative_path(
        &workspace.logical_root,
        &workspace.target.root,
        &path,
        canonical.as_deref(),
    )?;
    let relative = relative.to_str()?.replace('\\', "/");
    Some(topaz_resolve::normalize_path(&relative))
}

pub(super) fn logical_root_relative_path<'a>(
    logical_root: &Path,
    canonical_root: &Path,
    path: &'a Path,
    canonical: Option<&'a Path>,
) -> Option<&'a Path> {
    if canonical.is_some_and(|path| !path.starts_with(canonical_root)) {
        return None;
    }
    let relative = path
        .strip_prefix(logical_root)
        .ok()
        .or_else(|| path.strip_prefix(canonical_root).ok())
        .or_else(|| canonical.and_then(|path| path.strip_prefix(canonical_root).ok()))?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    if relative.components().any(|component| {
        !matches!(
            component,
            std::path::Component::CurDir | std::path::Component::Normal(_)
        )
    }) {
        return None;
    }
    Some(relative)
}

pub(super) fn lsp_checked_diagnostics(
    out: topaz_resolve::ResolveOutput,
    version: LangVersion,
    check_profile: Option<profile::CheckProfile>,
) -> Result<(SourceMap, Vec<LspPublishedDiagnostic>), String> {
    let check_profile = match check_profile {
        Some(profile::CheckProfile::Bootstrap) => {
            return Err(
                "check profile `bootstrap` applies to a locked package, not an LSP source session"
                    .to_string(),
            );
        }
        Some(profile::CheckProfile::TestProfile) => {
            return Err("LSP check profile must be `agent-pack`".to_string());
        }
        profile => profile,
    };
    let mut diagnostics = out.diagnostics.clone();
    if !has_errors(&diagnostics) {
        let unit = unit_modules(&out);
        let checked = topaz_check::check_unit_with_version(&unit, version);
        diagnostics.extend(checked.diagnostics);
    }
    let clean = !has_errors(&diagnostics);
    let mut diagnostics = diagnostics
        .into_iter()
        .map(|diagnostic| LspPublishedDiagnostic {
            diagnostic,
            profile_rule: None,
        })
        .collect::<Vec<_>>();
    if clean && check_profile == Some(profile::CheckProfile::AgentPack) {
        diagnostics.extend(
            profile::collect(&out, profile::CheckProfile::AgentPack)
                .into_iter()
                .map(|finding| LspPublishedDiagnostic {
                    diagnostic: finding.diagnostic,
                    profile_rule: finding.rule,
                }),
        );
    }
    Ok((out.map, diagnostics))
}

pub(super) fn push_lsp_diagnostic(
    out: &mut String,
    map: &SourceMap,
    published: &LspPublishedDiagnostic,
) {
    let diag = &published.diagnostic;
    let file = map.file(diag.primary.span.file);
    let (start_line, start_char) = lsp_position(file.src(), diag.primary.span.lo);
    let (end_line, end_char) = lsp_position(file.src(), diag.primary.span.hi);
    let severity = match diag.severity.as_str() {
        "warning" => 2,
        _ => 1,
    };
    out.push_str("{\"range\":{\"start\":{\"line\":");
    let _ = write!(
        out,
        "{start_line},\"character\":{start_char}}},\"end\":{{\"line\":{end_line},\"character\":{end_char}}}}}"
    );
    out.push_str(",\"severity\":");
    let _ = write!(out, "{severity}");
    out.push_str(",\"code\":");
    push_json_string(out, diag.code.as_str());
    out.push_str(",\"source\":\"topaz\",\"message\":");
    push_json_string(out, &diag.message);
    if let Some(profile_rule) = published.profile_rule {
        out.push_str(",\"data\":{\"profileRule\":");
        push_json_string(out, profile_rule);
        out.push('}');
    }
    out.push('}');
}

pub(super) fn lsp_position(src: &str, offset: u32) -> (u32, u32) {
    let end = (offset as usize).min(src.len());
    let prefix = if src.is_char_boundary(end) {
        &src[..end]
    } else {
        ""
    };
    let mut line = 0u32;
    let mut character = 0u32;
    for c in prefix.chars() {
        if c == '\n' {
            line += 1;
            character = 0;
        } else {
            character += c.len_utf16() as u32;
        }
    }
    (line, character)
}

pub(super) fn lsp_offset(src: &str, target_line: u32, target_character: u32) -> u32 {
    let mut line = 0u32;
    let mut character = 0u32;
    for (byte, c) in src.char_indices() {
        if line == target_line {
            if character >= target_character {
                return byte as u32;
            }
            if c == '\n' {
                return byte as u32;
            }
            let next = character + c.len_utf16() as u32;
            if target_character < next {
                return byte as u32;
            }
            character = next;
        } else if c == '\n' {
            line += 1;
            character = 0;
        }
    }
    src.len() as u32
}

pub(super) fn json_id_raw(input: &str) -> Option<String> {
    let (mut i, _) = json_field_value_start(input, "id")?;
    let bytes = input.as_bytes();
    if bytes.get(i) == Some(&b'"') {
        let (s, _) = parse_json_string(input, i)?;
        let mut out = String::new();
        push_json_string(&mut out, &s);
        return Some(out);
    }
    let start = i;
    while i < bytes.len() && !matches!(bytes[i], b',' | b'}' | b']' | b'\r' | b'\n' | b'\t' | b' ')
    {
        i += 1;
    }
    (i > start).then(|| input[start..i].to_string())
}

pub(super) fn json_u32_field(input: &str, key: &str) -> Option<u32> {
    let (mut i, _) = json_field_value_start(input, key)?;
    let bytes = input.as_bytes();
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    (i > start).then(|| input[start..i].parse().ok()).flatten()
}

pub(super) fn json_bool_field(input: &str, key: &str) -> Option<bool> {
    let (i, _) = json_field_value_start(input, key)?;
    if input[i..].starts_with("true") {
        Some(true)
    } else if input[i..].starts_with("false") {
        Some(false)
    } else {
        None
    }
}

pub(super) fn json_string_field(input: &str, key: &str) -> Option<String> {
    let (i, _) = json_field_value_start(input, key)?;
    parse_json_string(input, i).map(|(s, _)| s)
}

pub(super) fn json_field_value_start(input: &str, key: &str) -> Option<(usize, usize)> {
    let pattern = format!("\"{key}\"");
    let mut search = 0;
    while let Some(pos) = input[search..].find(&pattern) {
        let key_start = search + pos;
        let mut i = key_start + pattern.len();
        skip_json_ws(input, &mut i);
        if input.as_bytes().get(i) != Some(&b':') {
            search = key_start + 1;
            continue;
        }
        i += 1;
        skip_json_ws(input, &mut i);
        return Some((i, key_start));
    }
    None
}

pub(super) fn skip_json_ws(input: &str, i: &mut usize) {
    while input
        .as_bytes()
        .get(*i)
        .is_some_and(|b| matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
    {
        *i += 1;
    }
}

pub(super) fn parse_json_string(input: &str, start: usize) -> Option<(String, usize)> {
    let mut chars = input[start..].char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    while let Some((rel, c)) = chars.next() {
        let abs = start + rel;
        if escaped {
            match c {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'b' => out.push('\u{0008}'),
                'f' => out.push('\u{000c}'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let hex_start = abs + c.len_utf8();
                    let hex_end = hex_start + 4;
                    let hex = input.get(hex_start..hex_end)?;
                    let value = u16::from_str_radix(hex, 16).ok()?;
                    if (0xD800..=0xDBFF).contains(&value) {
                        let rest = input.get(hex_end..)?;
                        if !rest.starts_with("\\u") {
                            return None;
                        }
                        let low_hex = input.get(hex_end + 2..hex_end + 6)?;
                        let low = u16::from_str_radix(low_hex, 16).ok()?;
                        if !(0xDC00..=0xDFFF).contains(&low) {
                            return None;
                        }
                        let high_ten = (value as u32) - 0xD800;
                        let low_ten = (low as u32) - 0xDC00;
                        out.push(char::from_u32(0x10000 + ((high_ten << 10) | low_ten))?);
                        for _ in 0..10 {
                            chars.next();
                        }
                    } else {
                        out.push(char::from_u32(value as u32)?);
                        for _ in 0..4 {
                            chars.next();
                        }
                    }
                }
                _ => return None,
            }
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some((out, abs + c.len_utf8()));
        } else {
            out.push(c);
        }
    }
    None
}
