use crate::*;

pub(super) struct LspCompletionItem {
    pub(super) label: String,
    pub(super) kind: u32,
}

pub(super) struct LspMemberReceiver {
    pub(super) name: String,
    pub(super) span: Span,
    pub(super) dot: usize,
}

pub(super) fn lsp_completion_message(
    id: &str,
    text: &str,
    line: u32,
    character: u32,
    version: LangVersion,
) -> String {
    let items = lsp_completion_items(text, line, character, version);
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":[");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"label\":");
        push_json_string(&mut out, &item.label);
        out.push_str(",\"kind\":");
        let _ = write!(out, "{}", item.kind);
        out.push('}');
    }
    out.push_str("]}");
    out
}

pub(super) fn lsp_completion_items(
    text: &str,
    line: u32,
    character: u32,
    version: LangVersion,
) -> Vec<LspCompletionItem> {
    let mut items = BTreeMap::new();
    let member_completed =
        lsp_member_completion_receiver(text, line, character).is_some_and(|receiver| {
            if let Some(repaired) = lsp_repair_member_completion_source(text, receiver.dot)
                && let Some(definition) = lsp_definition_at(&repaired, receiver.span.lo, version)
            {
                lsp_insert_std_namespace_completions(
                    &mut items,
                    text,
                    version,
                    &receiver.name,
                    Some(definition),
                );
                lsp_insert_typed_receiver_completions(&mut items, &repaired, version, definition);
                true
            } else {
                lsp_insert_static_member_completions(&mut items, &receiver.name)
                    || lsp_insert_std_namespace_completions(
                        &mut items,
                        text,
                        version,
                        &receiver.name,
                        None,
                    )
            }
        });
    if !member_completed {
        if lsp_is_import_completion(text, line, character) {
            for module in topaz_resolve::std_module_identities() {
                insert_completion(&mut items, module, 9);
            }
        } else {
            for symbol in lsp_document_symbols(text, version) {
                insert_completion_symbol(&mut items, &symbol);
            }
            for span in lsp_identifier_candidate_spans(text) {
                if let Some(def) = lsp_definition_at(text, span.lo, version) {
                    insert_completion(&mut items, span_text(text, def), 6);
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
    items
        .into_iter()
        .map(|(label, kind)| LspCompletionItem { label, kind })
        .collect()
}

pub(super) fn lsp_member_completion_receiver(
    text: &str,
    line: u32,
    character: u32,
) -> Option<LspMemberReceiver> {
    let offset = lsp_offset(text, line, character) as usize;
    let bytes = text.as_bytes();
    let mut i = offset.min(bytes.len());
    while i > 0 {
        let b = bytes[i - 1];
        if b.is_ascii_alphanumeric() || b == b'_' {
            i -= 1;
        } else {
            break;
        }
    }
    if i == 0 || bytes.get(i - 1).copied() != Some(b'.') {
        return None;
    }
    let dot = i - 1;
    let mut start = dot;
    while start > 0 {
        let b = bytes[start - 1];
        if b.is_ascii_alphanumeric() || b == b'_' {
            start -= 1;
        } else {
            break;
        }
    }
    (start < dot).then(|| LspMemberReceiver {
        name: text[start..dot].to_string(),
        span: Span::new(FileId(0), start as u32, dot as u32),
        dot,
    })
}

pub(super) fn lsp_insert_static_member_completions(
    items: &mut BTreeMap<String, u32>,
    receiver: &str,
) -> bool {
    let before = items.len();
    for member in topaz_check::builtins::static_member_names(receiver) {
        insert_completion(items, member, 3);
    }
    for member in topaz_check::builtins::static_value_member_names(receiver) {
        insert_completion(items, member, 20);
    }
    for (_, member) in topaz_check::builtins::BUILTIN_PROTOCOL_SURFACES
        .iter()
        .filter(|(namespace, _)| *namespace == receiver)
    {
        insert_completion(items, member, 3);
    }
    items.len() > before
}

pub(super) fn lsp_insert_typed_receiver_completions(
    items: &mut BTreeMap<String, u32>,
    repaired: &str,
    version: LangVersion,
    def_span: Span,
) -> bool {
    let Some(checked) = lsp_checked_unit(repaired, version) else {
        return false;
    };
    let Some(hover) = checked
        .hover_types
        .into_iter()
        .find(|h| h.span.lo == def_span.lo && h.span.hi == def_span.hi)
    else {
        return false;
    };

    let before = items.len();
    for member in topaz_check::builtins::receiver_member_names(&hover.raw_ty) {
        let kind = match topaz_check::builtins::receiver_member(&hover.raw_ty, member) {
            Some(topaz_check::builtins::Member::Method(_)) => 2,
            Some(topaz_check::builtins::Member::Property(_)) => 10,
            None => continue,
        };
        insert_completion(items, member, kind);
    }
    items.len() > before
}

pub(super) fn lsp_repair_member_completion_source(src: &str, dot: usize) -> Option<String> {
    if src.as_bytes().get(dot).copied() != Some(b'.') {
        return None;
    }
    let mut repaired = String::with_capacity(src.len().saturating_sub(1));
    repaired.push_str(&src[..dot]);
    repaired.push_str(&src[dot + 1..]);
    Some(repaired)
}

pub(super) fn lsp_insert_std_namespace_completions(
    items: &mut BTreeMap<String, u32>,
    src: &str,
    version: LangVersion,
    receiver: &str,
    definition: Option<Span>,
) -> bool {
    let mut map = SourceMap::new();
    let Ok(file) = map.add_file("main.tpz", src.to_string()) else {
        return false;
    };
    let out = parse_with_options(
        file,
        map.file(file).src(),
        ParseOptions {
            language_version: version,
        },
    );
    let mut found = false;
    for stmt in &out.program.items {
        let ast::StmtKind::Import(import) = &stmt.kind else {
            continue;
        };
        let ast::ImportKind::Namespace { .. } = &import.kind else {
            continue;
        };
        let segments = lsp_import_segments(src, import);
        if segments.first().is_none_or(|seg| *seg != "std") {
            continue;
        }
        if lsp_namespace_import_binding(src, import).is_some_and(|(name, span)| {
            name == receiver && definition.is_none_or(|definition| definition == span)
        }) {
            found |= lsp_insert_std_module_completions(items, &segments, version);
        }
    }
    found
}

pub(super) fn lsp_insert_std_module_completions(
    items: &mut BTreeMap<String, u32>,
    segments: &[&str],
    version: LangVersion,
) -> bool {
    let Some((_, module_src)) = topaz_resolve::std_module_source(segments) else {
        return false;
    };
    let mut map = SourceMap::new();
    let Ok(file) = map.add_file(segments.join("."), module_src.to_string()) else {
        return false;
    };
    let out = parse_with_options(
        file,
        module_src,
        ParseOptions {
            language_version: version,
        },
    );
    if has_errors(&out.diagnostics) {
        return false;
    }
    let before = items.len();
    for stmt in &out.program.items {
        lsp_insert_std_export_completion(items, module_src, stmt);
    }
    items.len() > before
}

pub(super) fn lsp_insert_std_export_completion(
    items: &mut BTreeMap<String, u32>,
    src: &str,
    stmt: &ast::Stmt,
) {
    match &stmt.kind {
        ast::StmtKind::Export(inner) => lsp_insert_std_export_completion(items, src, inner),
        ast::StmtKind::Function(decl) => {
            insert_completion(items, span_text(src, decl.name.span), 3);
        }
        ast::StmtKind::TypeAlias(decl) => {
            insert_completion(items, span_text(src, decl.name.span), 7);
        }
        ast::StmtKind::Enum(decl) => {
            insert_completion(items, span_text(src, decl.name.span), 13);
        }
        ast::StmtKind::Record(decl) => {
            insert_completion(items, span_text(src, decl.name.span), 22);
        }
        ast::StmtKind::Newtype(decl) => {
            insert_completion(items, span_text(src, decl.name.span), 7);
        }
        _ => {}
    }
}

pub(super) fn lsp_is_import_completion(text: &str, line: u32, character: u32) -> bool {
    let offset = lsp_offset(text, line, character) as usize;
    let start = text[..offset.min(text.len())]
        .rfind('\n')
        .map_or(0, |pos| pos + 1);
    let prefix = &text[start..offset.min(text.len())];
    let trimmed = prefix.trim_start();
    trimmed.starts_with("import ") || trimmed == "import"
}

pub(super) fn insert_completion_symbol(
    items: &mut BTreeMap<String, u32>,
    symbol: &LspDocumentSymbol,
) {
    insert_completion(items, &symbol.name, completion_kind_for_symbol(symbol.kind));
    for child in &symbol.children {
        insert_completion_symbol(items, child);
    }
}

pub(super) fn insert_completion(items: &mut BTreeMap<String, u32>, label: &str, kind: u32) {
    if !label.is_empty() {
        items.entry(label.to_string()).or_insert(kind);
    }
}

pub(super) fn completion_kind_for_symbol(symbol_kind: u32) -> u32 {
    match symbol_kind {
        3 => 9,
        5 => 7,
        6 => 2,
        8 => 5,
        10 => 13,
        11 => 8,
        12 => 3,
        13 => 6,
        14 => 21,
        22 => 20,
        23 => 22,
        _ => 1,
    }
}

pub(super) const TOPAZ_COMPLETION_KEYWORDS: &[&str] = &[
    "function", "let", "const", "return", "if", "else", "match", "for", "in", "while", "loop",
    "break", "continue", "defer", "using", "import", "export", "type", "record", "enum", "newtype",
    "impl", "protocol",
];

pub(super) fn lsp_insert_global_builtin_completions(items: &mut BTreeMap<String, u32>) {
    for name in topaz_check::form::SOURCE_BUILTIN_TYPE_NAMES {
        insert_completion(items, name, if *name == "RoundingMode" { 13 } else { 7 });
    }
    for name in topaz_check::builtins::FREE_FUNCTION_NAMES {
        insert_completion(
            items,
            name,
            if matches!(*name, "Some" | "Ok" | "Err") {
                4
            } else {
                3
            },
        );
    }
    for name in topaz_check::builtins::CONSTANT_NAMES {
        insert_completion(items, name, 12);
    }
    for name in topaz_check::builtins::STATIC_NAMESPACE_NAMES {
        insert_completion(items, name, 9);
    }
    for (namespace, _) in topaz_check::builtins::BUILTIN_PROTOCOL_SURFACES {
        insert_completion(items, namespace, 8);
    }
}
