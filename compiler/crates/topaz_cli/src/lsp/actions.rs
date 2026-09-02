use crate::*;

pub(super) fn lsp_code_action_message(
    id: &str,
    uri: &str,
    text: &str,
    version: LangVersion,
) -> String {
    let (_, diagnostics) = lsp_diagnostics(text, version);
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":[");
    let mut first = true;
    for diagnostic in &diagnostics {
        let Some(replacement) = lsp_diagnostic_replacement(diagnostic) else {
            continue;
        };
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str("{\"title\":");
        let title = format!("Replace with `{replacement}`");
        push_json_string(&mut out, &title);
        out.push_str(",\"kind\":\"quickfix\",\"edit\":{\"changes\":{");
        push_json_string(&mut out, uri);
        out.push_str(":[");
        push_lsp_text_edit(&mut out, text, diagnostic.primary.span, &replacement);
        out.push_str("]}}}");
    }
    out.push_str("]}");
    out
}

pub(super) fn lsp_diagnostic_replacement(diagnostic: &Diagnostic) -> Option<String> {
    std::iter::once(diagnostic.message.as_str())
        .chain(diagnostic.notes.iter().map(String::as_str))
        .find_map(lsp_extract_did_you_mean)
        .filter(|name| lsp_rename_name_is_valid(name))
}

pub(super) fn lsp_extract_did_you_mean(message: &str) -> Option<String> {
    let marker = "did you mean `";
    let start = message.find(marker)? + marker.len();
    let end = message[start..].find('`')?;
    Some(message[start..start + end].to_string())
}

pub(super) fn push_lsp_text_edit(out: &mut String, text: &str, span: Span, new_text: &str) {
    out.push_str("{\"range\":");
    push_lsp_range(out, text, span);
    out.push_str(",\"newText\":");
    push_json_string(out, new_text);
    out.push('}');
}

pub(super) fn lsp_identifier_candidate_spans(text: &str) -> Vec<Span> {
    topaz_lexer::lex(FileId(0), text)
        .tokens
        .into_iter()
        .filter(|token| matches!(token.kind, topaz_syntax::TokenKind::Ident))
        .map(|token| token.span)
        .collect()
}

pub(super) fn push_lsp_location_result(out: &mut String, uri: &str, text: &str, span: Span) {
    out.push_str("{\"uri\":");
    push_json_string(out, uri);
    out.push_str(",\"range\":");
    push_lsp_range(out, text, span);
    out.push('}');
}

pub(super) fn push_lsp_location(out: &mut String, uri: &str, text: &str, span: Span) {
    out.push_str("{\"uri\":");
    push_json_string(out, uri);
    out.push_str(",\"range\":");
    push_lsp_range(out, text, span);
    out.push('}');
}

pub(super) fn lsp_definition_at(text: &str, offset: u32, version: LangVersion) -> Option<Span> {
    let mut map = SourceMap::new();
    let file = map.add_file("main.tpz", text).ok()?;
    let out = parse_with_options(
        file,
        map.file(file).src(),
        ParseOptions {
            language_version: version,
        },
    );
    if has_errors(&out.diagnostics) {
        return None;
    }
    let mut scopes = vec![collect_top_level_bindings(text, &out.program)];
    let mut search = LspDefinitionSearch {
        src: text,
        offset,
        result: None,
    };
    for stmt in &out.program.items {
        if search.visit_stmt(stmt, &mut scopes) {
            break;
        }
    }
    search.result
}

pub(super) fn collect_top_level_bindings(
    src: &str,
    program: &ast::Program,
) -> BTreeMap<String, Span> {
    let mut bindings = BTreeMap::new();
    for stmt in &program.items {
        collect_stmt_bindings(src, stmt, &mut bindings);
    }
    bindings
}

pub(super) fn collect_stmt_bindings(
    src: &str,
    stmt: &ast::Stmt,
    bindings: &mut BTreeMap<String, Span>,
) {
    match &stmt.kind {
        ast::StmtKind::Import(item) => collect_import_bindings(src, item, bindings),
        ast::StmtKind::Export(inner) => collect_stmt_bindings(src, inner, bindings),
        ast::StmtKind::Function(decl) => {
            insert_span_binding(src, bindings, decl.name.span);
        }
        ast::StmtKind::TypeAlias(decl) => {
            insert_span_binding(src, bindings, decl.name.span);
        }
        ast::StmtKind::Enum(decl) => {
            insert_span_binding(src, bindings, decl.name.span);
            for variant in &decl.variants {
                insert_span_binding(src, bindings, variant.name.span);
            }
        }
        ast::StmtKind::Record(decl) => {
            insert_span_binding(src, bindings, decl.name.span);
        }
        ast::StmtKind::Newtype(decl) => {
            insert_span_binding(src, bindings, decl.name.span);
        }
        ast::StmtKind::Impl(decl) => {
            insert_span_binding(src, bindings, decl.name.span);
            for method in &decl.methods {
                insert_span_binding(src, bindings, method.decl.name.span);
            }
        }
        ast::StmtKind::Protocol(decl) => {
            insert_span_binding(src, bindings, decl.name.span);
        }
        ast::StmtKind::Let { pattern, .. } => collect_pattern_bindings(src, pattern, bindings),
        ast::StmtKind::Const { name, .. } => {
            insert_span_binding(src, bindings, name.span);
        }
        _ => {}
    }
}

pub(super) fn collect_import_bindings(
    src: &str,
    item: &ast::ImportItem,
    bindings: &mut BTreeMap<String, Span>,
) {
    match &item.kind {
        ast::ImportKind::Namespace { alias } => {
            if let Some(alias) = alias {
                insert_span_binding(src, bindings, alias.span);
            } else if let Some(last) = item.path.segments.last() {
                insert_span_binding(src, bindings, last.span);
            }
        }
        ast::ImportKind::Selected { specs } => {
            for spec in specs {
                insert_span_binding(src, bindings, spec.alias.unwrap_or(spec.name).span);
            }
        }
    }
}

pub(super) fn collect_pattern_bindings(
    src: &str,
    pattern: &ast::Pattern,
    bindings: &mut BTreeMap<String, Span>,
) {
    match &pattern.kind {
        ast::PatternKind::Binding(name) | ast::PatternKind::Typed { name, .. } => {
            insert_span_binding(src, bindings, name.span);
        }
        ast::PatternKind::Or(alts) => {
            for alt in alts {
                collect_pattern_bindings(src, alt, bindings);
            }
        }
        ast::PatternKind::Constructor { args, .. } => {
            for arg in args {
                collect_pattern_bindings(src, arg, bindings);
            }
        }
        ast::PatternKind::List(elems) => {
            for elem in elems {
                match elem {
                    ast::ListPatternElem::Pattern(pattern)
                    | ast::ListPatternElem::Rest(Some(pattern)) => {
                        collect_pattern_bindings(src, pattern, bindings);
                    }
                    ast::ListPatternElem::Rest(None) => {}
                }
            }
        }
        ast::PatternKind::Record(fields) | ast::PatternKind::NominalRecord { fields, .. } => {
            for field in fields {
                if let Some(pattern) = &field.pattern {
                    collect_pattern_bindings(src, pattern, bindings);
                } else {
                    insert_span_binding(src, bindings, field.name.span);
                }
            }
        }
        ast::PatternKind::Wildcard
        | ast::PatternKind::Literal(_)
        | ast::PatternKind::Range { .. } => {}
    }
}

pub(super) fn insert_span_binding(src: &str, bindings: &mut BTreeMap<String, Span>, span: Span) {
    bindings.insert(span_text(src, span).to_string(), span);
}
