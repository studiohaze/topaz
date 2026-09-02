use crate::*;

pub(super) struct LspDocumentSymbol {
    pub(super) name: String,
    pub(super) kind: u32,
    pub(super) range: Span,
    pub(super) selection_range: Span,
    pub(super) children: Vec<LspDocumentSymbol>,
}

pub(super) fn lsp_document_symbol_message(id: &str, text: &str, version: LangVersion) -> String {
    let symbols = lsp_document_symbols(text, version);
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":[");
    for (i, symbol) in symbols.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_lsp_document_symbol(&mut out, text, symbol);
    }
    out.push_str("]}");
    out
}

pub(super) fn lsp_document_symbols(text: &str, version: LangVersion) -> Vec<LspDocumentSymbol> {
    let mut map = SourceMap::new();
    let Ok(file) = map.add_file("main.tpz", text) else {
        return Vec::new();
    };
    let out = parse_with_options(
        file,
        map.file(file).src(),
        ParseOptions {
            language_version: version,
        },
    );
    if has_errors(&out.diagnostics) {
        return Vec::new();
    }
    out.program
        .items
        .iter()
        .filter_map(|stmt| lsp_symbol_for_stmt(text, stmt))
        .collect()
}

pub(super) fn lsp_symbol_for_stmt(src: &str, stmt: &ast::Stmt) -> Option<LspDocumentSymbol> {
    match &stmt.kind {
        ast::StmtKind::Export(inner) => lsp_symbol_for_stmt(src, inner),
        ast::StmtKind::Function(decl) => Some(lsp_symbol(
            span_text(src, decl.name.span),
            12,
            stmt.span,
            decl.name.span,
            Vec::new(),
        )),
        ast::StmtKind::TypeAlias(decl) => Some(lsp_symbol(
            span_text(src, decl.name.span),
            5,
            stmt.span,
            decl.name.span,
            Vec::new(),
        )),
        ast::StmtKind::Enum(decl) => Some(lsp_symbol(
            span_text(src, decl.name.span),
            10,
            stmt.span,
            decl.name.span,
            decl.variants
                .iter()
                .map(|variant| {
                    lsp_symbol(
                        span_text(src, variant.name.span),
                        22,
                        variant.span,
                        variant.name.span,
                        Vec::new(),
                    )
                })
                .collect(),
        )),
        ast::StmtKind::Record(decl) => Some(lsp_symbol(
            span_text(src, decl.name.span),
            23,
            stmt.span,
            decl.name.span,
            decl.fields
                .iter()
                .map(|field| {
                    lsp_symbol(
                        span_text(src, field.name.span),
                        8,
                        field.span,
                        field.name.span,
                        Vec::new(),
                    )
                })
                .collect(),
        )),
        ast::StmtKind::Newtype(decl) => Some(lsp_symbol(
            span_text(src, decl.name.span),
            5,
            stmt.span,
            decl.name.span,
            Vec::new(),
        )),
        ast::StmtKind::Impl(decl) => {
            let head = span_text(src, decl.name.span);
            let name = if let Some(target) = decl.target {
                format!("impl {head}<{}>", span_text(src, target.span))
            } else {
                format!("impl {head}")
            };
            Some(lsp_symbol(
                &name,
                3,
                stmt.span,
                decl.name.span,
                decl.methods
                    .iter()
                    .map(|method| {
                        lsp_symbol(
                            span_text(src, method.decl.name.span),
                            6,
                            method.span,
                            method.decl.name.span,
                            Vec::new(),
                        )
                    })
                    .collect(),
            ))
        }
        ast::StmtKind::Protocol(decl) => Some(lsp_symbol(
            span_text(src, decl.name.span),
            11,
            stmt.span,
            decl.name.span,
            decl.methods
                .iter()
                .map(|method| {
                    lsp_symbol(
                        span_text(src, method.name.span),
                        6,
                        method.name.span,
                        method.name.span,
                        Vec::new(),
                    )
                })
                .collect(),
        )),
        ast::StmtKind::Let { pattern, .. } => {
            let (name, selection) = lsp_pattern_symbol_name(src, pattern)?;
            Some(lsp_symbol(&name, 13, stmt.span, selection, Vec::new()))
        }
        ast::StmtKind::Const { name, .. } => Some(lsp_symbol(
            span_text(src, name.span),
            14,
            stmt.span,
            name.span,
            Vec::new(),
        )),
        _ => None,
    }
}

pub(super) fn lsp_pattern_symbol_name(src: &str, pattern: &ast::Pattern) -> Option<(String, Span)> {
    match &pattern.kind {
        ast::PatternKind::Binding(name) | ast::PatternKind::Typed { name, .. } => {
            Some((span_text(src, name.span).to_string(), name.span))
        }
        _ => None,
    }
}

pub(super) fn lsp_symbol(
    name: &str,
    kind: u32,
    range: Span,
    selection_range: Span,
    children: Vec<LspDocumentSymbol>,
) -> LspDocumentSymbol {
    LspDocumentSymbol {
        name: name.to_string(),
        kind,
        range,
        selection_range,
        children,
    }
}

pub(super) fn push_lsp_document_symbol(out: &mut String, src: &str, symbol: &LspDocumentSymbol) {
    out.push_str("{\"name\":");
    push_json_string(out, &symbol.name);
    out.push_str(",\"kind\":");
    let _ = write!(out, "{}", symbol.kind);
    out.push_str(",\"range\":");
    push_lsp_range(out, src, symbol.range);
    out.push_str(",\"selectionRange\":");
    push_lsp_range(out, src, symbol.selection_range);
    if !symbol.children.is_empty() {
        out.push_str(",\"children\":[");
        for (i, child) in symbol.children.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            push_lsp_document_symbol(out, src, child);
        }
        out.push(']');
    }
    out.push('}');
}

pub(super) fn push_lsp_range(out: &mut String, src: &str, span: Span) {
    let (start_line, start_char) = lsp_position(src, span.lo);
    let (end_line, end_char) = lsp_position(src, span.hi);
    let _ = write!(
        out,
        "{{\"start\":{{\"line\":{start_line},\"character\":{start_char}}},\"end\":{{\"line\":{end_line},\"character\":{end_char}}}}}"
    );
}

pub(super) fn span_text(src: &str, span: Span) -> &str {
    src.get(span.lo as usize..span.hi as usize).unwrap_or("")
}

pub(super) fn lsp_definition_message(
    id: &str,
    uri: &str,
    text: &str,
    line: u32,
    character: u32,
    version: LangVersion,
) -> String {
    let offset = lsp_offset(text, line, character);
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":");
    let Some(span) = lsp_definition_at(text, offset, version) else {
        out.push_str("null}");
        return out;
    };
    push_lsp_location_result(&mut out, uri, text, span);
    out.push('}');
    out
}

pub(super) fn lsp_references_message(
    id: &str,
    uri: &str,
    text: &str,
    line: u32,
    character: u32,
    version: LangVersion,
    include_declaration: bool,
) -> String {
    let spans = lsp_references_at(text, line, character, version, include_declaration);
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":[");
    for (i, span) in spans.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_lsp_location(&mut out, uri, text, *span);
    }
    out.push_str("]}");
    out
}

pub(super) fn lsp_references_at(
    text: &str,
    line: u32,
    character: u32,
    version: LangVersion,
    include_declaration: bool,
) -> Vec<Span> {
    let offset = lsp_offset(text, line, character);
    let Some(target) = lsp_definition_at(text, offset, version) else {
        return Vec::new();
    };
    let mut spans: Vec<_> = lsp_identifier_candidate_spans(text)
        .into_iter()
        .filter(|span| include_declaration || *span != target)
        .filter(|span| lsp_definition_at(text, span.lo, version) == Some(target))
        .collect();
    spans.sort_by_key(|span| (span.lo, span.hi));
    spans.dedup_by_key(|span| (span.lo, span.hi));
    spans
}

pub(super) fn lsp_rename_message(
    id: &str,
    uri: &str,
    text: &str,
    line: u32,
    character: u32,
    version: LangVersion,
    new_name: &str,
) -> String {
    let mut out = format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":");
    if !lsp_rename_name_is_valid(new_name) {
        out.push_str("null}");
        return out;
    }
    let spans = lsp_references_at(text, line, character, version, true);
    if spans.is_empty() {
        out.push_str("null}");
        return out;
    }
    out.push_str("{\"changes\":{");
    push_json_string(&mut out, uri);
    out.push_str(":[");
    for (i, span) in spans.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_lsp_text_edit(&mut out, text, *span, new_name);
    }
    out.push_str("]}}}");
    out
}

pub(super) fn lsp_rename_name_is_valid(name: &str) -> bool {
    if name == "_" {
        return false;
    }
    let lexed = topaz_lexer::lex(FileId(0), name);
    if !lexed.diagnostics.is_empty() {
        return false;
    }
    let mut tokens = lexed
        .tokens
        .into_iter()
        .filter(|token| !matches!(token.kind, topaz_syntax::TokenKind::Eof));
    let Some(token) = tokens.next() else {
        return false;
    };
    tokens.next().is_none()
        && matches!(token.kind, topaz_syntax::TokenKind::Ident)
        && token.span.lo == 0
        && token.span.hi as usize == name.len()
}

pub(super) struct LspDefinitionSearch<'a> {
    pub(super) src: &'a str,
    pub(super) offset: u32,
    pub(super) result: Option<Span>,
}

impl LspDefinitionSearch<'_> {
    pub(super) fn visit_stmt(
        &mut self,
        stmt: &ast::Stmt,
        scopes: &mut Vec<BTreeMap<String, Span>>,
    ) -> bool {
        match &stmt.kind {
            ast::StmtKind::Import(item) => self.visit_import(item, scopes),
            ast::StmtKind::Export(inner) => self.visit_stmt(inner, scopes),
            ast::StmtKind::Function(decl) => self.visit_function_decl(decl, scopes),
            ast::StmtKind::TypeAlias(decl) => {
                self.bind_ident(decl.name, scopes)
                    || self.with_child_scope(scopes, |this, scopes| {
                        this.bind_idents(&decl.type_params, scopes)
                            || this.visit_type(&decl.ty, scopes)
                    })
            }
            ast::StmtKind::Enum(decl) => {
                self.bind_ident(decl.name, scopes)
                    || self.with_child_scope(scopes, |this, scopes| {
                        if this.bind_idents(&decl.type_params, scopes) {
                            return true;
                        }
                        for derive in &decl.derives {
                            if this.visit_ident_use(*derive, scopes) {
                                return true;
                            }
                        }
                        for variant in &decl.variants {
                            if this.bind_ident(variant.name, scopes) {
                                return true;
                            }
                            if let Some(payload) = &variant.payload {
                                for ty in payload {
                                    if this.visit_type(ty, scopes) {
                                        return true;
                                    }
                                }
                            }
                        }
                        false
                    })
            }
            ast::StmtKind::Record(decl) => {
                self.bind_ident(decl.name, scopes)
                    || self.with_child_scope(scopes, |this, scopes| {
                        if this.bind_idents(&decl.type_params, scopes) {
                            return true;
                        }
                        for derive in &decl.derives {
                            if this.visit_ident_use(*derive, scopes) {
                                return true;
                            }
                        }
                        for field in &decl.fields {
                            if this.visit_type(&field.ty, scopes) {
                                return true;
                            }
                            if let Some(default) = &field.default
                                && this.visit_expr(default, scopes)
                            {
                                return true;
                            }
                        }
                        false
                    })
            }
            ast::StmtKind::Newtype(decl) => {
                self.bind_ident(decl.name, scopes)
                    || self.with_child_scope(scopes, |this, scopes| {
                        this.bind_idents(&decl.type_params, scopes)
                            || this.visit_type(&decl.base, scopes)
                    })
            }
            ast::StmtKind::Impl(decl) => {
                if self.visit_ident_use(decl.name, scopes) {
                    return true;
                }
                if let Some(target) = decl.target
                    && self.visit_ident_use(target, scopes)
                {
                    return true;
                }
                for method in &decl.methods {
                    if self.visit_function_decl(&method.decl, scopes) {
                        return true;
                    }
                }
                false
            }
            ast::StmtKind::Protocol(decl) => {
                self.bind_ident(decl.name, scopes)
                    || self.with_child_scope(scopes, |this, scopes| {
                        if this.bind_idents(&decl.type_params, scopes) {
                            return true;
                        }
                        for method in &decl.methods {
                            if this.visit_function_decl(method, scopes) {
                                return true;
                            }
                        }
                        false
                    })
            }
            ast::StmtKind::Let {
                pattern, ty, value, ..
            } => {
                if let Some(ty) = ty
                    && self.visit_type(ty, scopes)
                {
                    return true;
                }
                self.visit_expr(value, scopes) || self.visit_pattern(pattern, scopes)
            }
            ast::StmtKind::Const { name, ty, value } => {
                if let Some(ty) = ty
                    && self.visit_type(ty, scopes)
                {
                    return true;
                }
                self.visit_expr(value, scopes) || self.bind_ident(*name, scopes)
            }
            ast::StmtKind::Assign { target, value, .. } => {
                self.visit_expr(target, scopes) || self.visit_expr(value, scopes)
            }
            ast::StmtKind::Return(value) => value
                .as_ref()
                .is_some_and(|value| self.visit_expr(value, scopes)),
            ast::StmtKind::Defer(expr) => self.visit_expr(expr, scopes),
            ast::StmtKind::Expr(expr) => self.visit_expr(expr, scopes),
            ast::StmtKind::Using { name, value, body } => {
                self.visit_expr(value, scopes)
                    || self.with_child_scope(scopes, |this, scopes| {
                        this.bind_ident(*name, scopes) || this.visit_block(body, scopes)
                    })
            }
            ast::StmtKind::While { cond, body } => {
                self.visit_expr(cond, scopes) || self.visit_block(body, scopes)
            }
            ast::StmtKind::Break { value, label } => {
                if let Some(label) = label
                    && self.contains(label.span)
                {
                    self.result = Some(label.span);
                    return true;
                }
                value
                    .as_ref()
                    .is_some_and(|value| self.visit_expr(value, scopes))
            }
            ast::StmtKind::Continue { label } => {
                if let Some(label) = label
                    && self.contains(label.span)
                {
                    self.result = Some(label.span);
                    return true;
                }
                false
            }
        }
    }

    pub(super) fn visit_import(
        &mut self,
        item: &ast::ImportItem,
        scopes: &mut [BTreeMap<String, Span>],
    ) -> bool {
        match &item.kind {
            ast::ImportKind::Namespace { alias } => {
                if let Some(alias) = alias {
                    self.bind_ident(*alias, scopes)
                } else if let Some(last) = item.path.segments.last() {
                    self.bind_ident(*last, scopes)
                } else {
                    false
                }
            }
            ast::ImportKind::Selected { specs } => {
                for spec in specs {
                    if self.bind_ident(spec.alias.unwrap_or(spec.name), scopes) {
                        return true;
                    }
                }
                false
            }
        }
    }

    pub(super) fn visit_function_decl(
        &mut self,
        decl: &ast::FunctionDecl,
        scopes: &mut Vec<BTreeMap<String, Span>>,
    ) -> bool {
        self.bind_ident(decl.name, scopes)
            || self.with_child_scope(scopes, |this, scopes| {
                if this.bind_idents(&decl.type_params, scopes) {
                    return true;
                }
                for bound_set in &decl.type_param_bounds {
                    for bound in bound_set {
                        if this.visit_ident_use(*bound, scopes) {
                            return true;
                        }
                    }
                }
                for param in &decl.params {
                    if this.visit_type(&param.ty, scopes)
                        || this.bind_ident(param.name, scopes)
                        || param
                            .default
                            .as_ref()
                            .is_some_and(|default| this.visit_expr(default, scopes))
                    {
                        return true;
                    }
                }
                if let Some(ret) = &decl.return_type
                    && this.visit_type(ret, scopes)
                {
                    return true;
                }
                this.visit_block(&decl.body, scopes)
            })
    }

    pub(super) fn visit_block(
        &mut self,
        block: &ast::Block,
        scopes: &mut Vec<BTreeMap<String, Span>>,
    ) -> bool {
        self.with_child_scope(scopes, |this, scopes| {
            for stmt in &block.stmts {
                if this.visit_stmt(stmt, scopes) {
                    return true;
                }
            }
            block
                .tail
                .as_ref()
                .is_some_and(|tail| this.visit_expr(tail, scopes))
        })
    }

    pub(super) fn visit_expr(
        &mut self,
        expr: &ast::Expr,
        scopes: &mut Vec<BTreeMap<String, Span>>,
    ) -> bool {
        match &expr.kind {
            ast::ExprKind::Ident => self.visit_span_use(expr.span, scopes),
            ast::ExprKind::String(lit) => {
                for part in &lit.parts {
                    if let ast::StringPart::Interpolation(expr) = part
                        && self.visit_expr(expr, scopes)
                    {
                        return true;
                    }
                }
                false
            }
            ast::ExprKind::Paren(inner) | ast::ExprKind::Try(inner) => {
                self.visit_expr(inner, scopes)
            }
            ast::ExprKind::Block(block) => self.visit_block(block, scopes),
            ast::ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.visit_expr(cond, scopes)
                    || self.visit_block(then_block, scopes)
                    || else_branch
                        .as_ref()
                        .is_some_and(|else_branch| self.visit_expr(else_branch, scopes))
            }
            ast::ExprKind::Match { scrutinee, cases } => {
                if self.visit_expr(scrutinee, scopes) {
                    return true;
                }
                for case in cases {
                    if self.with_child_scope(scopes, |this, scopes| {
                        this.visit_pattern(&case.pattern, scopes)
                            || case
                                .guard
                                .as_ref()
                                .is_some_and(|guard| this.visit_expr(guard, scopes))
                            || match &case.body {
                                ast::CaseArmBody::Expr(expr) => this.visit_expr(expr, scopes),
                                ast::CaseArmBody::Return { value, .. } => value
                                    .as_ref()
                                    .is_some_and(|value| this.visit_expr(value, scopes)),
                            }
                    }) {
                        return true;
                    }
                }
                false
            }
            ast::ExprKind::For {
                pattern,
                iter,
                body,
            } => {
                self.visit_expr(iter, scopes)
                    || self.with_child_scope(scopes, |this, scopes| {
                        this.visit_pattern(pattern, scopes) || this.visit_block(body, scopes)
                    })
            }
            ast::ExprKind::Loop { label, body } => {
                if let Some(label) = label
                    && self.contains(label.span)
                {
                    self.result = Some(label.span);
                    return true;
                }
                self.visit_block(body, scopes)
            }
            ast::ExprKind::Concurrent {
                timeout,
                arms,
                else_block,
            } => {
                if timeout
                    .as_ref()
                    .is_some_and(|timeout| self.visit_expr(timeout, scopes))
                {
                    return true;
                }
                for arm in arms {
                    if self.bind_ident(arm.name, scopes) || self.visit_expr(&arm.value, scopes) {
                        return true;
                    }
                }
                else_block
                    .as_ref()
                    .is_some_and(|else_block| self.visit_block(else_block, scopes))
            }
            ast::ExprKind::Call {
                callee,
                args,
                type_args,
            } => {
                if self.visit_expr(callee, scopes) {
                    return true;
                }
                for ty in type_args {
                    if self.visit_type(ty, scopes) {
                        return true;
                    }
                }
                for arg in args {
                    if self.visit_call_arg(arg, scopes) {
                        return true;
                    }
                }
                false
            }
            ast::ExprKind::Member { object, field }
            | ast::ExprKind::OptionalAccess { object, field } => {
                self.visit_expr(object, scopes) || self.visit_ident_use(*field, scopes)
            }
            ast::ExprKind::Index { object, index } => {
                self.visit_expr(object, scopes) || self.visit_expr(index, scopes)
            }
            ast::ExprKind::Unary { operand, .. } => self.visit_expr(operand, scopes),
            ast::ExprKind::Binary { lhs, rhs, .. } | ast::ExprKind::Compose { lhs, rhs } => {
                self.visit_expr(lhs, scopes) || self.visit_expr(rhs, scopes)
            }
            ast::ExprKind::Range { lo, hi, step, .. } => {
                self.visit_expr(lo, scopes)
                    || self.visit_expr(hi, scopes)
                    || step
                        .as_ref()
                        .is_some_and(|step| self.visit_expr(step, scopes))
            }
            ast::ExprKind::Pipe { lhs, rhs } => {
                self.visit_expr(lhs, scopes)
                    || match rhs.as_ref() {
                        ast::PipeRhs::Expr(expr) => self.visit_expr(expr, scopes),
                        ast::PipeRhs::Field(field) => self.visit_ident_use(*field, scopes),
                    }
            }
            ast::ExprKind::Lambda { params, body } => {
                self.with_child_scope(scopes, |this, scopes| {
                    for param in params {
                        if param
                            .ty
                            .as_ref()
                            .is_some_and(|ty| this.visit_type(ty, scopes))
                            || this.bind_ident(param.name, scopes)
                        {
                            return true;
                        }
                    }
                    this.visit_expr(body, scopes)
                })
            }
            ast::ExprKind::RecordLiteral { fields } => self.visit_field_inits(fields, scopes),
            ast::ExprKind::RecordUpdate {
                base,
                spread,
                fields,
            } => {
                self.visit_expr(base, scopes)
                    || spread
                        .as_ref()
                        .is_some_and(|spread| self.visit_expr(spread, scopes))
                    || self.visit_field_inits(fields, scopes)
            }
            ast::ExprKind::Array(elems) => {
                for elem in elems {
                    let found = match elem {
                        ast::ArrayElement::Expr(expr) | ast::ArrayElement::Spread(expr) => {
                            self.visit_expr(expr, scopes)
                        }
                    };
                    if found {
                        return true;
                    }
                }
                false
            }
            ast::ExprKind::SetLiteral(elems) => {
                for elem in elems {
                    if self.visit_expr(elem, scopes) {
                        return true;
                    }
                }
                false
            }
            ast::ExprKind::MapLiteral(entries) => {
                for (key, value) in entries {
                    if self.visit_expr(key, scopes) || self.visit_expr(value, scopes) {
                        return true;
                    }
                }
                false
            }
            ast::ExprKind::Comprehension { clauses, body, .. } => {
                self.with_child_scope(scopes, |this, scopes| {
                    for clause in clauses {
                        match clause {
                            ast::CompClause::For { pattern, iter } => {
                                if this.visit_expr(iter, scopes)
                                    || this.visit_pattern(pattern, scopes)
                                {
                                    return true;
                                }
                            }
                            ast::CompClause::If(cond) => {
                                if this.visit_expr(cond, scopes) {
                                    return true;
                                }
                            }
                        }
                    }
                    match body.as_ref() {
                        ast::CompBody::Elem(expr) => this.visit_expr(expr, scopes),
                        ast::CompBody::Entry { key, value } => {
                            this.visit_expr(key, scopes) || this.visit_expr(value, scopes)
                        }
                    }
                })
            }
            ast::ExprKind::Int
            | ast::ExprKind::Float
            | ast::ExprKind::Duration(_)
            | ast::ExprKind::Bool(_)
            | ast::ExprKind::Null
            | ast::ExprKind::Unit
            | ast::ExprKind::Placeholder => false,
        }
    }

    pub(super) fn visit_call_arg(
        &mut self,
        arg: &ast::CallArg,
        scopes: &mut Vec<BTreeMap<String, Span>>,
    ) -> bool {
        match arg {
            ast::CallArg::Positional(expr) | ast::CallArg::Spread(expr) => {
                self.visit_expr(expr, scopes)
            }
            ast::CallArg::Named { value, .. } => self.visit_expr(value, scopes),
        }
    }

    pub(super) fn visit_field_inits(
        &mut self,
        fields: &[ast::FieldInit],
        scopes: &mut Vec<BTreeMap<String, Span>>,
    ) -> bool {
        for field in fields {
            if self.visit_expr(&field.value, scopes) {
                return true;
            }
        }
        false
    }

    pub(super) fn visit_pattern(
        &mut self,
        pattern: &ast::Pattern,
        scopes: &mut Vec<BTreeMap<String, Span>>,
    ) -> bool {
        match &pattern.kind {
            ast::PatternKind::Binding(name) | ast::PatternKind::Typed { name, .. } => {
                self.bind_ident(*name, scopes)
            }
            ast::PatternKind::Or(alts) => {
                for alt in alts {
                    if self.visit_pattern(alt, scopes) {
                        return true;
                    }
                }
                false
            }
            ast::PatternKind::Literal(expr) => self.visit_expr(expr, scopes),
            ast::PatternKind::Range { lo, hi, .. } => {
                self.visit_expr(lo, scopes) || self.visit_expr(hi, scopes)
            }
            ast::PatternKind::Constructor { name, args } => {
                if self.visit_ident_use(*name, scopes) {
                    return true;
                }
                for arg in args {
                    if self.visit_pattern(arg, scopes) {
                        return true;
                    }
                }
                false
            }
            ast::PatternKind::List(elems) => {
                for elem in elems {
                    let found = match elem {
                        ast::ListPatternElem::Pattern(pattern)
                        | ast::ListPatternElem::Rest(Some(pattern)) => {
                            self.visit_pattern(pattern, scopes)
                        }
                        ast::ListPatternElem::Rest(None) => false,
                    };
                    if found {
                        return true;
                    }
                }
                false
            }
            ast::PatternKind::Record(fields) => self.visit_record_pattern_fields(fields, scopes),
            ast::PatternKind::NominalRecord { name, fields } => {
                self.visit_ident_use(*name, scopes)
                    || self.visit_record_pattern_fields(fields, scopes)
            }
            ast::PatternKind::Wildcard => false,
        }
    }

    pub(super) fn visit_record_pattern_fields(
        &mut self,
        fields: &[ast::RecordPatternField],
        scopes: &mut Vec<BTreeMap<String, Span>>,
    ) -> bool {
        for field in fields {
            let found = if let Some(pattern) = &field.pattern {
                self.visit_pattern(pattern, scopes)
            } else {
                self.bind_ident(field.name, scopes)
            };
            if found {
                return true;
            }
        }
        false
    }

    pub(super) fn visit_type(
        &mut self,
        ty: &ast::Type,
        scopes: &mut Vec<BTreeMap<String, Span>>,
    ) -> bool {
        match &ty.kind {
            ast::TypeKind::Named { name, args } => {
                self.visit_ident_use(*name, scopes)
                    || args.iter().any(|ty| self.visit_type(ty, scopes))
            }
            ast::TypeKind::Qualified { ns, args, .. } => {
                self.visit_ident_use(*ns, scopes)
                    || args.iter().any(|ty| self.visit_type(ty, scopes))
            }
            ast::TypeKind::Record(fields) => {
                for field in fields {
                    if self.visit_type(&field.ty, scopes) {
                        return true;
                    }
                }
                false
            }
            ast::TypeKind::Function { params, ret } => {
                for param in params {
                    if self.visit_type(&param.ty, scopes) {
                        return true;
                    }
                }
                self.visit_type(ret, scopes)
            }
            ast::TypeKind::Union(types) => self.visit_types(types, scopes),
            ast::TypeKind::Literal | ast::TypeKind::Unit => false,
        }
    }

    pub(super) fn visit_types(
        &mut self,
        types: &[ast::Type],
        scopes: &mut Vec<BTreeMap<String, Span>>,
    ) -> bool {
        for ty in types {
            if self.visit_type(ty, scopes) {
                return true;
            }
        }
        false
    }

    pub(super) fn bind_idents(
        &mut self,
        idents: &[ast::Ident],
        scopes: &mut [BTreeMap<String, Span>],
    ) -> bool {
        for ident in idents {
            if self.bind_ident(*ident, scopes) {
                return true;
            }
        }
        false
    }

    pub(super) fn bind_ident(
        &mut self,
        ident: ast::Ident,
        scopes: &mut [BTreeMap<String, Span>],
    ) -> bool {
        if self.contains(ident.span) {
            self.result = Some(ident.span);
            return true;
        }
        scopes
            .last_mut()
            .expect("definition search always has a scope")
            .insert(span_text(self.src, ident.span).to_string(), ident.span);
        false
    }

    pub(super) fn visit_ident_use(
        &mut self,
        ident: ast::Ident,
        scopes: &[BTreeMap<String, Span>],
    ) -> bool {
        self.visit_span_use(ident.span, scopes)
    }

    pub(super) fn visit_span_use(&mut self, span: Span, scopes: &[BTreeMap<String, Span>]) -> bool {
        if !self.contains(span) {
            return false;
        }
        let name = span_text(self.src, span);
        self.result = scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied());
        true
    }

    pub(super) fn contains(&self, span: Span) -> bool {
        span.lo <= self.offset && self.offset < span.hi
    }

    pub(super) fn with_child_scope(
        &mut self,
        scopes: &mut Vec<BTreeMap<String, Span>>,
        f: impl FnOnce(&mut Self, &mut Vec<BTreeMap<String, Span>>) -> bool,
    ) -> bool {
        scopes.push(BTreeMap::new());
        let found = f(self, scopes);
        scopes.pop();
        found
    }
}

pub(super) fn lsp_diagnostics(text: &str, version: LangVersion) -> (SourceMap, Vec<Diagnostic>) {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", text);
    let out = resolve_with_version(&provider, "main.tpz", None, version);
    let (map, diagnostics) = lsp_checked_diagnostics(out, version, None)
        .expect("an unprofiled Rust LSP check cannot reject a profile");
    (
        map,
        diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.diagnostic)
            .collect(),
    )
}
