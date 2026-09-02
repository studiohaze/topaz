use crate::*;

pub(super) struct LspSignature {
    pub(super) label: String,
    pub(super) parameters: Vec<String>,
}

pub(super) struct LspCallTarget {
    pub(super) name: String,
    pub(super) span: Span,
    pub(super) cursor: usize,
    pub(super) active_parameter: u32,
}

pub(super) fn lsp_signature_help_message(
    id: &str,
    text: &str,
    line: u32,
    character: u32,
    version: LangVersion,
) -> String {
    let offset = lsp_offset(text, line, character);
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":");
    let Some(call) = lsp_call_name_before(text, offset) else {
        out.push_str("null}");
        return out;
    };
    let Some(signature) = lsp_signature_for_call(text, version, &call) else {
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
    for (i, parameter) in signature.parameters.iter().enumerate() {
        if i > 0 {
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

pub(super) fn lsp_call_name_before(text: &str, offset: u32) -> Option<LspCallTarget> {
    let bytes = text.as_bytes();
    let mut i = (offset as usize).min(bytes.len());
    let cursor = i;
    let mut depth = 0u32;
    let mut active_parameter = 0u32;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' if depth > 0 => depth -= 1,
            b',' if depth == 0 => active_parameter += 1,
            b'(' if depth == 0 => {
                let mut end = i;
                while end > 0 && bytes[end - 1].is_ascii_whitespace() {
                    end -= 1;
                }
                let mut start = end;
                while start > 0 {
                    let b = bytes[start - 1];
                    if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.') {
                        start -= 1;
                    } else {
                        break;
                    }
                }
                return (start < end).then(|| LspCallTarget {
                    name: text[start..end].to_string(),
                    span: Span::new(FileId(0), start as u32, end as u32),
                    cursor,
                    active_parameter,
                });
            }
            _ => {}
        }
    }
    None
}

pub(super) fn lsp_signature_for_call(
    text: &str,
    version: LangVersion,
    call: &LspCallTarget,
) -> Option<LspSignature> {
    if let Some(signature) = lsp_signature_for_receiver_member(text, version, call) {
        return Some(signature);
    }
    let source_definition = lsp_repair_call_source(text, call)
        .and_then(|repaired| lsp_definition_at(&repaired, call.span.lo, version));
    let mut map = SourceMap::new();
    let file = map.add_file("main.tpz", text).ok()?;
    let out = parse_with_options(
        file,
        map.file(file).src(),
        ParseOptions {
            language_version: version,
        },
    );
    if let Some(signature) = out
        .program
        .items
        .iter()
        .find_map(|stmt| lsp_signature_for_stmt(text, stmt, &call.name, source_definition))
    {
        return Some(signature);
    }
    if let Some(signature) =
        lsp_signature_from_std_imports(text, version, &out.program, &call.name, source_definition)
    {
        return Some(signature);
    }
    if source_definition.is_some() {
        None
    } else {
        lsp_signature_for_builtin(&call.name)
    }
}

pub(super) fn lsp_signature_for_builtin(name: &str) -> Option<LspSignature> {
    if let Some(scheme) = topaz_check::builtins::free_function(name) {
        return Some(lsp_signature_from_scheme(name, &scheme));
    }
    let (head, member) = name.rsplit_once('.')?;
    topaz_check::builtins::static_member(head, member)
        .map(|scheme| lsp_signature_from_scheme(name, &scheme))
}

pub(super) fn lsp_signature_for_receiver_member(
    src: &str,
    version: LangVersion,
    call: &LspCallTarget,
) -> Option<LspSignature> {
    let (receiver_name, member) = call.name.rsplit_once('.')?;
    let receiver_hi = call.span.lo.checked_add(receiver_name.len() as u32)?;
    if receiver_hi >= call.span.hi {
        return None;
    }
    let dot = receiver_hi as usize;
    let repaired = lsp_repair_member_call_source(src, dot, call.cursor)?;
    let def_span = lsp_definition_at(&repaired, call.span.lo, version)?;
    let checked = lsp_checked_unit(&repaired, version)?;
    let hover = checked
        .hover_types
        .into_iter()
        .find(|h| h.span.lo == def_span.lo && h.span.hi == def_span.hi)?;
    match topaz_check::builtins::receiver_member(&hover.raw_ty, member)? {
        topaz_check::builtins::Member::Method(scheme) => {
            Some(lsp_signature_from_scheme(&call.name, &scheme))
        }
        topaz_check::builtins::Member::Property(_) => None,
    }
}

pub(super) fn lsp_repair_member_call_source(
    src: &str,
    dot: usize,
    cursor: usize,
) -> Option<String> {
    if cursor < dot || src.as_bytes().get(dot).copied() != Some(b'.') {
        return None;
    }
    let mut repaired = String::with_capacity(src.len().saturating_sub(cursor - dot));
    repaired.push_str(&src[..dot]);
    repaired.push_str(&src[cursor..]);
    Some(repaired)
}

pub(super) fn lsp_repair_call_source(src: &str, call: &LspCallTarget) -> Option<String> {
    if call.cursor < call.span.hi as usize
        || src.as_bytes().get(call.span.hi as usize).copied() != Some(b'(')
    {
        return None;
    }
    let removed = call.cursor - call.span.hi as usize;
    let mut repaired = String::with_capacity(src.len().saturating_sub(removed));
    repaired.push_str(&src[..call.span.hi as usize]);
    repaired.push_str(&src[call.cursor..]);
    Some(repaired)
}

pub(super) fn lsp_signature_for_stmt(
    src: &str,
    stmt: &ast::Stmt,
    name: &str,
    definition: Option<Span>,
) -> Option<LspSignature> {
    match &stmt.kind {
        ast::StmtKind::Export(inner) => lsp_signature_for_stmt(src, inner, name, definition),
        ast::StmtKind::Function(decl)
            if span_text(src, decl.name.span) == name
                && definition.is_none_or(|definition| definition == decl.name.span) =>
        {
            Some(lsp_signature_for_function(src, decl, name))
        }
        _ => None,
    }
}

pub(super) fn lsp_signature_for_function(
    src: &str,
    decl: &ast::FunctionDecl,
    display_name: &str,
) -> LspSignature {
    let mut label = String::new();
    label.push_str(display_name);
    label.push('(');
    let mut parameters = Vec::new();
    for (i, param) in decl.params.iter().enumerate() {
        if i > 0 {
            label.push_str(", ");
        }
        let mut parameter = String::new();
        if param.variadic {
            parameter.push_str("...");
        }
        parameter.push_str(span_text(src, param.name.span));
        parameter.push_str(": ");
        parameter.push_str(span_text(src, param.ty.span));
        label.push_str(&parameter);
        parameters.push(parameter);
    }
    label.push_str(") -> ");
    if let Some(ret) = &decl.return_type {
        label.push_str(span_text(src, ret.span));
    } else {
        label.push_str("()");
    }
    LspSignature { label, parameters }
}

pub(super) fn lsp_signature_from_std_imports(
    src: &str,
    version: LangVersion,
    program: &ast::Program,
    name: &str,
    definition: Option<Span>,
) -> Option<LspSignature> {
    if let Some((namespace, member)) = name.rsplit_once('.') {
        for stmt in &program.items {
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
            let (binding, binding_span) = lsp_namespace_import_binding(src, import)?;
            if binding == namespace
                && definition.is_none_or(|definition| definition == binding_span)
            {
                return lsp_signature_from_std_module(&segments, member, name, version);
            }
        }
        return None;
    }

    for stmt in &program.items {
        let ast::StmtKind::Import(import) = &stmt.kind else {
            continue;
        };
        let ast::ImportKind::Selected { specs } = &import.kind else {
            continue;
        };
        let segments = lsp_import_segments(src, import);
        if segments.first().is_none_or(|seg| *seg != "std") {
            continue;
        }
        for spec in specs {
            let member = span_text(src, spec.name.span);
            let (binding, binding_span) = spec
                .alias
                .as_ref()
                .map(|alias| (span_text(src, alias.span), alias.span))
                .unwrap_or((member, spec.name.span));
            if binding == name && definition.is_none_or(|definition| definition == binding_span) {
                return lsp_signature_from_std_module(&segments, member, name, version);
            }
        }
    }
    None
}

pub(super) fn lsp_import_segments<'a>(src: &'a str, import: &ast::ImportItem) -> Vec<&'a str> {
    import
        .path
        .segments
        .iter()
        .map(|segment| span_text(src, segment.span))
        .collect()
}

pub(super) fn lsp_namespace_import_binding<'a>(
    src: &'a str,
    import: &ast::ImportItem,
) -> Option<(&'a str, Span)> {
    let ast::ImportKind::Namespace { alias } = &import.kind else {
        return None;
    };
    alias
        .as_ref()
        .map(|alias| (span_text(src, alias.span), alias.span))
        .or_else(|| {
            import
                .path
                .segments
                .last()
                .map(|segment| (span_text(src, segment.span), segment.span))
        })
}

pub(super) fn lsp_signature_from_std_module(
    segments: &[&str],
    member: &str,
    display_name: &str,
    version: LangVersion,
) -> Option<LspSignature> {
    let (_, module_src) = topaz_resolve::std_module_source(segments)?;
    let mut map = SourceMap::new();
    let file = map
        .add_file(segments.join("."), module_src.to_string())
        .ok()?;
    let out = parse_with_options(
        file,
        module_src,
        ParseOptions {
            language_version: version,
        },
    );
    if has_errors(&out.diagnostics) {
        return None;
    }
    out.program
        .items
        .iter()
        .find_map(|stmt| lsp_signature_for_std_stmt(module_src, stmt, member, display_name))
}

pub(super) fn lsp_signature_for_std_stmt(
    src: &str,
    stmt: &ast::Stmt,
    member: &str,
    display_name: &str,
) -> Option<LspSignature> {
    match &stmt.kind {
        ast::StmtKind::Export(inner) => {
            lsp_signature_for_std_stmt(src, inner, member, display_name)
        }
        ast::StmtKind::Function(decl) if span_text(src, decl.name.span) == member => {
            Some(lsp_signature_for_function(src, decl, display_name))
        }
        _ => None,
    }
}

pub(super) fn lsp_signature_from_scheme(
    display_name: &str,
    scheme: &topaz_check::builtins::Scheme,
) -> LspSignature {
    let var_names = lsp_scheme_var_names(display_name, scheme.vars);
    let mut label = String::new();
    label.push_str(display_name);
    label.push('(');
    let mut parameters = Vec::new();
    for (i, ty) in scheme.params.iter().enumerate() {
        if i > 0 {
            label.push_str(", ");
        }
        let param = format!(
            "{}: {}",
            scheme.names.get(i).map(String::as_str).unwrap_or("arg"),
            lsp_type_label(ty, &var_names)
        );
        label.push_str(&param);
        parameters.push(param);
    }
    if let Some(variadic) = &scheme.variadic {
        if !scheme.params.is_empty() {
            label.push_str(", ");
        }
        let name = scheme
            .names
            .get(scheme.params.len())
            .map(String::as_str)
            .unwrap_or("value");
        let param = format!("...{name}: {}", lsp_type_label(variadic, &var_names));
        label.push_str(&param);
        parameters.push(param);
    }
    label.push_str(") -> ");
    label.push_str(&lsp_type_label(&scheme.ret, &var_names));
    LspSignature { label, parameters }
}

pub(super) fn lsp_scheme_var_names(display_name: &str, count: u32) -> Vec<String> {
    let preferred: &[&str] = if display_name.starts_with("Map.") {
        &["K", "V", "T", "U"]
    } else if matches!(display_name, "Ok" | "Err")
        || display_name.ends_with("assertOk")
        || display_name.ends_with("assertErr")
    {
        &["T", "E", "U", "V"]
    } else {
        &["T", "U", "V", "W", "X", "Y", "Z"]
    };
    (0..count)
        .map(|i| {
            preferred
                .get(i as usize)
                .map(|name| (*name).to_string())
                .unwrap_or_else(|| format!("T{i}"))
        })
        .collect()
}

pub(super) fn lsp_type_label(ty: &topaz_check::Type, var_names: &[String]) -> String {
    match ty {
        topaz_check::Type::Prim(p) => p.name().to_string(),
        topaz_check::Type::Literal(lit) => match lit {
            topaz_check::Lit::Str(s) => format!("\"{s}\""),
            topaz_check::Lit::Int(n) => n.to_string(),
            topaz_check::Lit::Float(s) => s.clone(),
            topaz_check::Lit::Bool(b) => b.to_string(),
            topaz_check::Lit::Null => "null".to_string(),
        },
        topaz_check::Type::Union(members) => members
            .iter()
            .map(|member| lsp_type_label(member, var_names))
            .collect::<Vec<_>>()
            .join(" | "),
        topaz_check::Type::Record(fields) => {
            let mut out = String::from("{ ");
            for (i, (name, field_ty)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let _ = write!(out, "{name}: {}", lsp_type_label(field_ty, var_names));
            }
            out.push_str(" }");
            out
        }
        topaz_check::Type::Ctor(topaz_check::Ctor::Range, _) => "range".to_string(),
        topaz_check::Type::Ctor(ctor, args) => {
            let mut out = String::from(ctor.name());
            out.push('<');
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&lsp_type_label(arg, var_names));
            }
            out.push('>');
            out
        }
        topaz_check::Type::Func {
            params,
            variadic,
            ret,
        } => {
            let mut out = String::from("(");
            for (i, param) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&lsp_type_label(param, var_names));
            }
            if let Some(variadic) = variadic {
                if !params.is_empty() {
                    out.push_str(", ");
                }
                out.push_str("...");
                out.push_str(&lsp_type_label(variadic, var_names));
            }
            out.push_str(") -> ");
            out.push_str(&lsp_type_label(ret, var_names));
            out
        }
        topaz_check::Type::Foreign { name, args } => {
            let mut out = name.clone();
            if !args.is_empty() {
                out.push('<');
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&lsp_type_label(arg, var_names));
                }
                out.push('>');
            }
            out
        }
        topaz_check::Type::Skolem { name, .. } => name.clone(),
        topaz_check::Type::Template => "template".to_string(),
        topaz_check::Type::File => "File".to_string(),
        topaz_check::Type::JsonValue => "JSONValue".to_string(),
        topaz_check::Type::Bytes => "Bytes".to_string(),
        topaz_check::Type::ByteBuffer => "ByteBuffer".to_string(),
        topaz_check::Type::Path => "Path".to_string(),
        topaz_check::Type::Regex => "Regex".to_string(),
        topaz_check::Type::Match => "Match".to_string(),
        topaz_check::Type::TomlValue => "TOMLValue".to_string(),
        topaz_check::Type::Url => "URL".to_string(),
        topaz_check::Type::Date => "Date".to_string(),
        topaz_check::Type::BigInt => "BigInt".to_string(),
        topaz_check::Type::Decimal => "Decimal".to_string(),
        topaz_check::Type::RoundingMode => "RoundingMode".to_string(),
        topaz_check::Type::Enum { .. }
        | topaz_check::Type::NominalRecord { .. }
        | topaz_check::Type::Newtype { .. } => ty.to_string(),
        topaz_check::Type::Unknown => "?".to_string(),
        topaz_check::Type::Var(i) => var_names
            .get(*i as usize)
            .cloned()
            .unwrap_or_else(|| format!("T{i}")),
    }
}
