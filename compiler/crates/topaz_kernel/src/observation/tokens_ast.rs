use super::*;

/// Maps a lexer token to the stable spelling used in observation rows.
pub fn canonical_token_kind(kind: TokenKind) -> String {
    match kind {
        TokenKind::Kw(keyword) => return format!("keyword/{}", keyword.as_str()),
        TokenKind::Duration(unit) => return format!("duration/{}", duration_unit(unit)),
        TokenKind::StringStart { tagged, multiline } => {
            return format!("string-start/tagged={tagged}/multiline={multiline}");
        }
        TokenKind::Ident => "identifier",
        TokenKind::Underscore => "underscore",
        TokenKind::Int => "integer",
        TokenKind::Float => "float",
        TokenKind::Label => "label",
        TokenKind::StringText => "string-text",
        TokenKind::InterpolationStart => "interpolation-start",
        TokenKind::InterpolationEnd => "interpolation-end",
        TokenKind::StringEnd => "string-end",
        TokenKind::LParen => "left-paren",
        TokenKind::RParen => "right-paren",
        TokenKind::LBracket => "left-bracket",
        TokenKind::RBracket => "right-bracket",
        TokenKind::LBrace => "left-brace",
        TokenKind::RBrace => "right-brace",
        TokenKind::Plus => "plus",
        TokenKind::Minus => "minus",
        TokenKind::Star => "star",
        TokenKind::Slash => "slash",
        TokenKind::Percent => "percent",
        TokenKind::StarStar => "star-star",
        TokenKind::DotDot => "dot-dot",
        TokenKind::DotDotLt => "dot-dot-lt",
        TokenKind::Ellipsis => "ellipsis",
        TokenKind::Dot => "dot",
        TokenKind::QuestionDot => "question-dot",
        TokenKind::Question => "question",
        TokenKind::QuestionQuestion => "question-question",
        TokenKind::QuestionQuestionEq => "question-question-eq",
        TokenKind::Lt => "lt",
        TokenKind::Le => "le",
        TokenKind::Gt => "gt",
        TokenKind::Ge => "ge",
        TokenKind::GtGt => "gt-gt",
        TokenKind::EqEq => "eq-eq",
        TokenKind::Ne => "not-eq",
        TokenKind::Eq => "eq",
        TokenKind::PlusEq => "plus-eq",
        TokenKind::MinusEq => "minus-eq",
        TokenKind::StarEq => "star-eq",
        TokenKind::SlashEq => "slash-eq",
        TokenKind::PercentEq => "percent-eq",
        TokenKind::Bang => "bang",
        TokenKind::Tilde => "tilde",
        TokenKind::AndAnd => "and-and",
        TokenKind::OrOr => "or-or",
        TokenKind::Pipe => "pipe",
        TokenKind::PipeGt => "pipe-gt",
        TokenKind::FatArrow => "fat-arrow",
        TokenKind::ThinArrow => "thin-arrow",
        TokenKind::Comma => "comma",
        TokenKind::Colon => "colon",
        TokenKind::Semicolon => "semicolon",
        TokenKind::Sep => "separator",
        TokenKind::Newline => "newline",
        TokenKind::Eof => "eof",
    }
    .to_string()
}

fn duration_unit(unit: DurationUnit) -> &'static str {
    match unit {
        DurationUnit::Ms => "ms",
        DurationUnit::S => "s",
        DurationUnit::M => "m",
    }
}

pub(super) fn token_rows(
    identity: &SourceIdentity,
    src: &str,
    stream: &str,
    tokens: &[Token],
) -> Vec<JsonValue> {
    tokens
        .iter()
        .enumerate()
        .map(|(ordinal, token)| {
            let spelling = if token.span.lo <= token.span.hi
                && usize::try_from(token.span.hi).is_ok_and(|hi| hi <= src.len())
            {
                &src[token.span.lo as usize..token.span.hi as usize]
            } else {
                ""
            };
            object([
                ("kind", string(canonical_token_kind(token.kind))),
                ("ordinal", unsigned(ordinal as u64)),
                ("schema", string(TOKENS_SCHEMA)),
                ("sourceId", string(&identity.source_id)),
                ("sourceOrdinal", unsigned(identity.ordinal)),
                ("span", span(&identity.source_id, token.span)),
                ("spelling", string(spelling)),
                ("stream", string(stream)),
                (
                    "synthetic",
                    boolean(matches!(token.kind, TokenKind::Sep) && spelling != ";"),
                ),
            ])
        })
        .collect()
}

pub(super) struct AstProjector<'a> {
    source_id: &'a str,
    src: &'a str,
    next: u64,
    pub(super) rows: Vec<JsonValue>,
    pub(super) identity_nodes: BTreeMap<(u32, u32), String>,
    record: bool,
}

impl<'a> AstProjector<'a> {
    pub(super) fn new(source_id: &'a str, src: &'a str) -> Self {
        Self {
            source_id,
            src,
            next: 0,
            rows: Vec::new(),
            identity_nodes: BTreeMap::new(),
            record: true,
        }
    }

    fn counting(source_id: &'a str, src: &'a str) -> Self {
        Self {
            source_id,
            src,
            next: 0,
            rows: Vec::new(),
            identity_nodes: BTreeMap::new(),
            record: false,
        }
    }

    fn push(
        &mut self,
        kind: &str,
        value_span: Span,
        parent: Option<&str>,
        field: &str,
        index: u64,
        attributes: impl IntoIterator<Item = (&'static str, JsonValue)>,
    ) -> String {
        let id = format!("{}#n{:08x}", self.source_id, self.next);
        self.next += 1;
        if !self.record {
            return id;
        }
        self.identity_nodes
            .entry((value_span.lo, value_span.hi))
            .or_insert_with(|| id.clone());
        let spelling = if matches!(
            kind,
            "identifier"
                | "expression/integer"
                | "expression/float"
                | "expression/duration"
                | "expression/boolean"
                | "expression/null"
                | "expression/unit"
                | "expression/string"
                | "expression/template"
                | "expression/identifier"
                | "pattern/wildcard"
                | "template-tag"
                | "string-part/text"
                | "type/literal"
                | "type/unit"
        ) {
            self.src
                .get(value_span.lo as usize..value_span.hi as usize)
                .unwrap_or("")
        } else {
            ""
        };
        let mut fields = BTreeMap::<String, JsonValue>::from([
            ("field".to_string(), string(field)),
            ("index".to_string(), unsigned(index)),
            ("kind".to_string(), string(kind)),
            ("nodeId".to_string(), string(&id)),
            (
                "parentNodeId".to_string(),
                parent.map_or(JsonValue::Null, string),
            ),
            ("schema".to_string(), string(AST_SCHEMA)),
            ("sourceId".to_string(), string(self.source_id)),
            ("span".to_string(), span(self.source_id, value_span)),
            ("spelling".to_string(), string(spelling)),
        ]);
        for (key, value) in attributes {
            fields.insert(key.to_string(), value);
        }
        self.rows.push(object(fields));
        id
    }

    fn ident(&mut self, ident: Ident, parent: &str, field: &str, index: u64) {
        self.push("identifier", ident.span, Some(parent), field, index, []);
    }

    pub(super) fn program(&mut self, program: &Program) {
        let id = self.push("program", program.span, None, "root", 0, []);
        for (index, statement) in program.items.iter().enumerate() {
            self.stmt(statement, &id, "items", index as u64);
        }
    }

    fn stmt(&mut self, statement: &Stmt, parent: &str, field: &str, index: u64) {
        let kind = match &statement.kind {
            StmtKind::Import(_) => "statement/import",
            StmtKind::Export(_) => "statement/export",
            StmtKind::Function(_) => "statement/function",
            StmtKind::TypeAlias(_) => "statement/type-alias",
            StmtKind::Enum(_) => "statement/enum",
            StmtKind::Record(_) => "statement/record",
            StmtKind::Newtype(_) => "statement/newtype",
            StmtKind::Impl(_) => "statement/impl",
            StmtKind::Protocol(_) => "statement/protocol",
            StmtKind::Let { .. } => "statement/let",
            StmtKind::Const { .. } => "statement/const",
            StmtKind::Assign { .. } => "statement/assign",
            StmtKind::Return(_) => "statement/return",
            StmtKind::Defer(_) => "statement/defer",
            StmtKind::Using { .. } => "statement/using",
            StmtKind::While { .. } => "statement/while",
            StmtKind::Break { .. } => "statement/break",
            StmtKind::Continue { .. } => "statement/continue",
            StmtKind::Expr(_) => "statement/expression",
        };
        let attributes = match &statement.kind {
            StmtKind::Let { mutable, .. } => vec![("mutable", boolean(*mutable))],
            StmtKind::Assign { op, .. } => vec![("operator", string(assign_op(*op)))],
            _ => Vec::new(),
        };
        let id = self.push(kind, statement.span, Some(parent), field, index, attributes);
        match &statement.kind {
            StmtKind::Import(import) => self.import(import, &id),
            StmtKind::Export(inner) => self.stmt(inner, &id, "item", 0),
            StmtKind::Function(declaration) => self.function(declaration, &id, "declaration", 0),
            StmtKind::TypeAlias(alias) => {
                self.ident(alias.name, &id, "name", 0);
                for (index, param) in alias.type_params.iter().enumerate() {
                    self.ident(*param, &id, "typeParameters", index as u64);
                }
                self.ty(&alias.ty, &id, "type", 0);
            }
            StmtKind::Enum(declaration) => self.enum_decl(declaration, &id),
            StmtKind::Record(declaration) => self.record_decl(declaration, &id),
            StmtKind::Newtype(declaration) => {
                self.ident(declaration.name, &id, "name", 0);
                for (index, param) in declaration.type_params.iter().enumerate() {
                    self.ident(*param, &id, "typeParameters", index as u64);
                }
                self.ty(&declaration.base, &id, "base", 0);
            }
            StmtKind::Impl(declaration) => self.impl_decl(declaration, &id),
            StmtKind::Protocol(declaration) => {
                self.ident(declaration.name, &id, "name", 0);
                for (index, param) in declaration.type_params.iter().enumerate() {
                    self.ident(*param, &id, "typeParameters", index as u64);
                }
                for (index, method) in declaration.methods.iter().enumerate() {
                    self.function(method, &id, "methods", index as u64);
                }
            }
            StmtKind::Let {
                pattern, ty, value, ..
            } => {
                self.pattern(pattern, &id, "pattern", 0);
                if let Some(ty) = ty {
                    self.ty(ty, &id, "type", 0);
                }
                self.expr(value, &id, "value", 0);
            }
            StmtKind::Const { name, ty, value } => {
                self.ident(*name, &id, "name", 0);
                if let Some(ty) = ty {
                    self.ty(ty, &id, "type", 0);
                }
                self.expr(value, &id, "value", 0);
            }
            StmtKind::Assign { target, value, .. } => {
                self.expr(target, &id, "target", 0);
                self.expr(value, &id, "value", 0);
            }
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.expr(value, &id, "value", 0);
                }
            }
            StmtKind::Defer(value) => {
                self.expr(value, &id, "value", 0);
            }
            StmtKind::Expr(value) => {
                self.expr(value, &id, "value", 0);
            }
            StmtKind::Using { name, value, body } => {
                self.ident(*name, &id, "name", 0);
                self.expr(value, &id, "value", 0);
                self.block(body, &id, "body", 0);
            }
            StmtKind::While { cond, body } => {
                self.expr(cond, &id, "condition", 0);
                self.block(body, &id, "body", 0);
            }
            StmtKind::Break { label, value } => {
                if let Some(label) = label {
                    self.ident(*label, &id, "label", 0);
                }
                if let Some(value) = value {
                    self.expr(value, &id, "value", 0);
                }
            }
            StmtKind::Continue { label } => {
                if let Some(label) = label {
                    self.ident(*label, &id, "label", 0);
                }
            }
        }
    }

    fn import(&mut self, import: &ImportItem, parent: &str) {
        let id = self.push("import-item", import.span, Some(parent), "import", 0, []);
        let path = self.push("module-path", import.path.span, Some(&id), "path", 0, []);
        for (index, segment) in import.path.segments.iter().enumerate() {
            self.ident(*segment, &path, "segments", index as u64);
        }
        match &import.kind {
            ImportKind::Namespace { alias } => {
                if let Some(alias) = alias {
                    self.ident(*alias, &id, "alias", 0);
                }
            }
            ImportKind::Selected { specs } => {
                for (index, spec) in specs.iter().enumerate() {
                    let spec_id = self.push(
                        "import-spec",
                        spec.span,
                        Some(&id),
                        "specs",
                        index as u64,
                        [],
                    );
                    self.ident(spec.name, &spec_id, "name", 0);
                    if let Some(alias) = spec.alias {
                        self.ident(alias, &spec_id, "alias", 0);
                    }
                }
            }
        }
    }

    fn function(&mut self, declaration: &FunctionDecl, parent: &str, field: &str, index: u64) {
        let value_span = declaration.name.span.merge(declaration.body.span);
        let id = self.push(
            "function-declaration",
            value_span,
            Some(parent),
            field,
            index,
            [],
        );
        self.ident(declaration.name, &id, "name", 0);
        for (index, param) in declaration.type_params.iter().enumerate() {
            self.ident(*param, &id, "typeParameters", index as u64);
        }
        for (param_index, bounds) in declaration.type_param_bounds.iter().enumerate() {
            for (bound_index, bound) in bounds.iter().enumerate() {
                self.ident(
                    *bound,
                    &id,
                    "typeParameterBounds",
                    ((param_index as u64) << 32) | bound_index as u64,
                );
            }
        }
        for (index, parameter) in declaration.params.iter().enumerate() {
            let param_id = self.push(
                "parameter",
                parameter.span,
                Some(&id),
                "parameters",
                index as u64,
                [("variadic", boolean(parameter.variadic))],
            );
            self.ident(parameter.name, &param_id, "name", 0);
            self.ty(&parameter.ty, &param_id, "type", 0);
            if let Some(default) = &parameter.default {
                self.expr(default, &param_id, "default", 0);
            }
        }
        if let Some(return_type) = &declaration.return_type {
            self.ty(return_type, &id, "returnType", 0);
        }
        self.block(&declaration.body, &id, "body", 0);
    }

    fn enum_decl(&mut self, declaration: &EnumDecl, parent: &str) {
        self.ident(declaration.name, parent, "name", 0);
        for (index, param) in declaration.type_params.iter().enumerate() {
            self.ident(*param, parent, "typeParameters", index as u64);
        }
        for (index, variant) in declaration.variants.iter().enumerate() {
            let id = self.push(
                "enum-variant",
                variant.span,
                Some(parent),
                "variants",
                index as u64,
                [],
            );
            self.ident(variant.name, &id, "name", 0);
            if let Some(payload) = &variant.payload {
                for (index, ty) in payload.iter().enumerate() {
                    self.ty(ty, &id, "payload", index as u64);
                }
            }
        }
        for (index, derives) in declaration.derives.iter().enumerate() {
            self.ident(*derives, parent, "derives", index as u64);
        }
    }

    fn record_decl(&mut self, declaration: &RecordDecl, parent: &str) {
        self.ident(declaration.name, parent, "name", 0);
        for (index, param) in declaration.type_params.iter().enumerate() {
            self.ident(*param, parent, "typeParameters", index as u64);
        }
        for (index, field) in declaration.fields.iter().enumerate() {
            let id = self.push(
                "record-field-declaration",
                field.span,
                Some(parent),
                "fields",
                index as u64,
                [],
            );
            self.ident(field.name, &id, "name", 0);
            self.ty(&field.ty, &id, "type", 0);
            if let Some(default) = &field.default {
                self.expr(default, &id, "default", 0);
            }
        }
        for (index, derives) in declaration.derives.iter().enumerate() {
            self.ident(*derives, parent, "derives", index as u64);
        }
    }

    fn impl_decl(&mut self, declaration: &ImplDecl, parent: &str) {
        self.ident(declaration.name, parent, "name", 0);
        if let Some(target) = declaration.target {
            self.ident(target, parent, "target", 0);
        }
        for (index, method) in declaration.methods.iter().enumerate() {
            let id = self.push(
                "impl-method",
                method.span,
                Some(parent),
                "methods",
                index as u64,
                [("exported", boolean(method.exported))],
            );
            self.function(&method.decl, &id, "declaration", 0);
        }
    }

    fn block(&mut self, block: &Block, parent: &str, field: &str, index: u64) {
        let id = self.push("block", block.span, Some(parent), field, index, []);
        for (index, statement) in block.stmts.iter().enumerate() {
            self.stmt(statement, &id, "statements", index as u64);
        }
        if let Some(tail) = &block.tail {
            self.expr(tail, &id, "tail", 0);
        }
    }

    fn expr(&mut self, expression: &Expr, parent: &str, field: &str, index: u64) {
        let source_spelling = self
            .src
            .get(expression.span.lo as usize..expression.span.hi as usize)
            .unwrap_or("");
        let (kind, attributes) = match &expression.kind {
            ExprKind::Int => (
                "expression/integer",
                vec![("valueDecimal", string(source_spelling.replace('_', "")))],
            ),
            ExprKind::Float => (
                "expression/float",
                vec![(
                    "floatBits",
                    source_spelling
                        .parse::<f64>()
                        .ok()
                        .filter(|value| value.is_finite())
                        .map(|value| string(format!("{:016x}", value.to_bits())))
                        .unwrap_or(JsonValue::Null),
                )],
            ),
            ExprKind::Duration(unit) => (
                "expression/duration",
                vec![
                    ("unit", string(duration_unit(*unit))),
                    (
                        "valueDecimal",
                        source_spelling
                            .strip_suffix(duration_unit(*unit))
                            .map(|value| string(value.replace('_', "")))
                            .unwrap_or(JsonValue::Null),
                    ),
                ],
            ),
            ExprKind::Bool(value) => ("expression/boolean", vec![("value", boolean(*value))]),
            ExprKind::Null => ("expression/null", Vec::new()),
            ExprKind::Unit => ("expression/unit", Vec::new()),
            ExprKind::String(literal) if literal.tag.is_some() => {
                ("expression/template", Vec::new())
            }
            ExprKind::String(_) => ("expression/string", Vec::new()),
            ExprKind::Ident => ("expression/identifier", Vec::new()),
            ExprKind::Placeholder => ("expression/placeholder", Vec::new()),
            ExprKind::Paren(_) => ("expression/parenthesized", Vec::new()),
            ExprKind::Block(_) => ("expression/block", Vec::new()),
            ExprKind::If { .. } => ("expression/if", Vec::new()),
            ExprKind::Match { .. } => ("expression/match", Vec::new()),
            ExprKind::For { .. } => ("expression/for", Vec::new()),
            ExprKind::Loop { .. } => ("expression/loop", Vec::new()),
            ExprKind::Concurrent { .. } => ("expression/concurrent", Vec::new()),
            ExprKind::Call { .. } => ("expression/call", Vec::new()),
            ExprKind::Member { .. } => ("expression/member", Vec::new()),
            ExprKind::Index { .. } => ("expression/index", Vec::new()),
            ExprKind::OptionalAccess { .. } => ("expression/optional-access", Vec::new()),
            ExprKind::Try(_) => ("expression/try", Vec::new()),
            ExprKind::Unary { op, .. } => (
                "expression/unary",
                vec![("operator", string(unary_op(*op)))],
            ),
            ExprKind::Binary { op, .. } => (
                "expression/binary",
                vec![("operator", string(binary_op(*op)))],
            ),
            ExprKind::Range { inclusive, .. } => {
                ("expression/range", vec![("inclusive", boolean(*inclusive))])
            }
            ExprKind::Compose { .. } => ("expression/compose", Vec::new()),
            ExprKind::Pipe { .. } => ("expression/pipe", Vec::new()),
            ExprKind::Lambda { .. } => ("expression/lambda", Vec::new()),
            ExprKind::RecordLiteral { .. } => ("expression/record-literal", Vec::new()),
            ExprKind::RecordUpdate { .. } => ("expression/record-update", Vec::new()),
            ExprKind::Array(_) => ("expression/array", Vec::new()),
            ExprKind::SetLiteral(_) => ("expression/set", Vec::new()),
            ExprKind::MapLiteral(_) => ("expression/map", Vec::new()),
            ExprKind::Comprehension { kind, .. } => (
                "expression/comprehension",
                vec![("collection", string(comp_kind(*kind)))],
            ),
        };
        let id = self.push(
            kind,
            expression.span,
            Some(parent),
            field,
            index,
            attributes,
        );
        match &expression.kind {
            ExprKind::Int
            | ExprKind::Float
            | ExprKind::Duration(_)
            | ExprKind::Bool(_)
            | ExprKind::Null
            | ExprKind::Unit
            | ExprKind::Ident
            | ExprKind::Placeholder => {}
            ExprKind::String(literal) => self.string_lit(literal, &id),
            ExprKind::Paren(inner) | ExprKind::Try(inner) => {
                self.expr(inner, &id, "operand", 0);
            }
            ExprKind::Block(block) => self.block(block, &id, "block", 0),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.expr(cond, &id, "condition", 0);
                self.block(then_block, &id, "then", 0);
                if let Some(else_branch) = else_branch {
                    self.expr(else_branch, &id, "else", 0);
                }
            }
            ExprKind::Match { scrutinee, cases } => {
                self.expr(scrutinee, &id, "scrutinee", 0);
                for (index, case) in cases.iter().enumerate() {
                    let case_id = self.push(
                        "match-case",
                        case.span,
                        Some(&id),
                        "cases",
                        index as u64,
                        [],
                    );
                    self.pattern(&case.pattern, &case_id, "pattern", 0);
                    if let Some(guard) = &case.guard {
                        self.expr(guard, &case_id, "guard", 0);
                    }
                    match &case.body {
                        CaseArmBody::Expr(body) => self.expr(body, &case_id, "body", 0),
                        CaseArmBody::Return { value, span } => {
                            let return_id = self.push(
                                "match-case-return",
                                *span,
                                Some(&case_id),
                                "body",
                                0,
                                [],
                            );
                            if let Some(value) = value {
                                self.expr(value, &return_id, "value", 0);
                            }
                        }
                    }
                }
            }
            ExprKind::For {
                pattern,
                iter,
                body,
            } => {
                self.pattern(pattern, &id, "pattern", 0);
                self.expr(iter, &id, "iterator", 0);
                self.block(body, &id, "body", 0);
            }
            ExprKind::Loop { label, body } => {
                if let Some(label) = label {
                    self.ident(*label, &id, "label", 0);
                }
                self.block(body, &id, "body", 0);
            }
            ExprKind::Concurrent {
                timeout,
                arms,
                else_block,
            } => {
                if let Some(timeout) = timeout {
                    self.expr(timeout, &id, "timeout", 0);
                }
                for (index, arm) in arms.iter().enumerate() {
                    let arm_id = self.push(
                        "concurrent-arm",
                        arm.span,
                        Some(&id),
                        "arms",
                        index as u64,
                        [],
                    );
                    self.ident(arm.name, &arm_id, "name", 0);
                    self.expr(&arm.value, &arm_id, "value", 0);
                }
                if let Some(else_block) = else_block {
                    self.block(else_block, &id, "else", 0);
                }
            }
            ExprKind::Call {
                callee,
                args,
                type_args,
            } => {
                self.expr(callee, &id, "callee", 0);
                for (index, argument) in args.iter().enumerate() {
                    match argument {
                        CallArg::Positional(value) => {
                            self.expr(value, &id, "arguments", index as u64);
                        }
                        CallArg::Spread(value) => {
                            let arg_id = self.push(
                                "call-argument/spread",
                                value.span,
                                Some(&id),
                                "arguments",
                                index as u64,
                                [],
                            );
                            self.expr(value, &arg_id, "value", 0);
                        }
                        CallArg::Named { name, value } => {
                            let arg_id = self.push(
                                "call-argument/named",
                                name.span.merge(value.span),
                                Some(&id),
                                "arguments",
                                index as u64,
                                [],
                            );
                            self.ident(*name, &arg_id, "name", 0);
                            self.expr(value, &arg_id, "value", 0);
                        }
                    }
                }
                for (index, ty) in type_args.iter().enumerate() {
                    self.ty(ty, &id, "typeArguments", index as u64);
                }
            }
            ExprKind::Member { object, field } | ExprKind::OptionalAccess { object, field } => {
                self.expr(object, &id, "object", 0);
                self.ident(*field, &id, "field", 0);
            }
            ExprKind::Index { object, index } => {
                self.expr(object, &id, "object", 0);
                self.expr(index, &id, "index", 0);
            }
            ExprKind::Unary { operand, .. } => self.expr(operand, &id, "operand", 0),
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::Compose { lhs, rhs } => {
                self.expr(lhs, &id, "left", 0);
                self.expr(rhs, &id, "right", 0);
            }
            ExprKind::Range { lo, hi, step, .. } => {
                self.expr(lo, &id, "low", 0);
                self.expr(hi, &id, "high", 0);
                if let Some(step) = step {
                    self.expr(step, &id, "step", 0);
                }
            }
            ExprKind::Pipe { lhs, rhs } => {
                self.expr(lhs, &id, "left", 0);
                match rhs.as_ref() {
                    PipeRhs::Expr(rhs) => self.expr(rhs, &id, "right", 0),
                    PipeRhs::Field(field) => self.ident(*field, &id, "field", 0),
                }
            }
            ExprKind::Lambda { params, body } => {
                for (index, parameter) in params.iter().enumerate() {
                    let param_id = self.push(
                        "lambda-parameter",
                        parameter.span,
                        Some(&id),
                        "parameters",
                        index as u64,
                        [],
                    );
                    self.ident(parameter.name, &param_id, "name", 0);
                    if let Some(ty) = &parameter.ty {
                        self.ty(ty, &param_id, "type", 0);
                    }
                }
                self.expr(body, &id, "body", 0);
            }
            ExprKind::RecordLiteral { fields } => self.field_inits(fields, &id),
            ExprKind::RecordUpdate {
                base,
                spread,
                fields,
            } => {
                self.expr(base, &id, "base", 0);
                if let Some(spread) = spread {
                    self.expr(spread, &id, "spread", 0);
                }
                self.field_inits(fields, &id);
            }
            ExprKind::Array(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    match element {
                        ArrayElement::Expr(value) => {
                            self.expr(value, &id, "elements", index as u64);
                        }
                        ArrayElement::Spread(value) => {
                            let element_id = self.push(
                                "array-element/spread",
                                value.span,
                                Some(&id),
                                "elements",
                                index as u64,
                                [],
                            );
                            self.expr(value, &element_id, "value", 0);
                        }
                    }
                }
            }
            ExprKind::SetLiteral(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    self.expr(element, &id, "elements", index as u64);
                }
            }
            ExprKind::MapLiteral(entries) => {
                for (index, (key, value)) in entries.iter().enumerate() {
                    let entry_id = self.push(
                        "map-entry",
                        key.span.merge(value.span),
                        Some(&id),
                        "entries",
                        index as u64,
                        [],
                    );
                    self.expr(key, &entry_id, "key", 0);
                    self.expr(value, &entry_id, "value", 0);
                }
            }
            ExprKind::Comprehension { clauses, body, .. } => {
                for (index, clause) in clauses.iter().enumerate() {
                    match clause {
                        CompClause::For { pattern, iter } => {
                            let clause_id = self.push(
                                "comprehension-clause/for",
                                pattern.span.merge(iter.span),
                                Some(&id),
                                "clauses",
                                index as u64,
                                [],
                            );
                            self.pattern(pattern, &clause_id, "pattern", 0);
                            self.expr(iter, &clause_id, "iterator", 0);
                        }
                        CompClause::If(condition) => {
                            let clause_id = self.push(
                                "comprehension-clause/if",
                                condition.span,
                                Some(&id),
                                "clauses",
                                index as u64,
                                [],
                            );
                            self.expr(condition, &clause_id, "condition", 0);
                        }
                    }
                }
                match body.as_ref() {
                    CompBody::Elem(value) => self.expr(value, &id, "body", 0),
                    CompBody::Entry { key, value } => {
                        self.expr(key, &id, "bodyKey", 0);
                        self.expr(value, &id, "bodyValue", 0);
                    }
                }
            }
        }
    }

    fn field_inits(&mut self, fields: &[FieldInit], parent: &str) {
        for (index, field) in fields.iter().enumerate() {
            let id = self.push(
                "field-initializer",
                field.span,
                Some(parent),
                "fields",
                index as u64,
                [],
            );
            self.ident(field.name, &id, "name", 0);
            self.expr(&field.value, &id, "value", 0);
        }
    }

    fn string_lit(&mut self, literal: &StringLit, parent: &str) {
        if let Some(tag) = literal.tag {
            self.push("template-tag", tag, Some(parent), "tag", 0, []);
        }
        for (index, part) in literal.parts.iter().enumerate() {
            match part {
                StringPart::Text(value_span) => {
                    self.push(
                        "string-part/text",
                        *value_span,
                        Some(parent),
                        "parts",
                        index as u64,
                        [],
                    );
                }
                StringPart::Interpolation(value) => {
                    let id = self.push(
                        "string-part/interpolation",
                        value.span,
                        Some(parent),
                        "parts",
                        index as u64,
                        [],
                    );
                    self.expr(value, &id, "value", 0);
                }
            }
        }
    }

    fn pattern(&mut self, pattern: &Pattern, parent: &str, field: &str, index: u64) {
        let (kind, attributes) = match &pattern.kind {
            PatternKind::Or(_) => ("pattern/or", Vec::new()),
            PatternKind::Wildcard => ("pattern/wildcard", Vec::new()),
            PatternKind::Literal(_) => ("pattern/literal", Vec::new()),
            PatternKind::Range { inclusive, .. } => {
                ("pattern/range", vec![("inclusive", boolean(*inclusive))])
            }
            PatternKind::Binding(_) => ("pattern/binding", Vec::new()),
            PatternKind::Typed { .. } => ("pattern/typed", Vec::new()),
            PatternKind::Constructor { .. } => ("pattern/constructor", Vec::new()),
            PatternKind::List(_) => ("pattern/list", Vec::new()),
            PatternKind::Record(_) => ("pattern/record", Vec::new()),
            PatternKind::NominalRecord { .. } => ("pattern/nominal-record", Vec::new()),
        };
        let id = self.push(kind, pattern.span, Some(parent), field, index, attributes);
        match &pattern.kind {
            PatternKind::Or(alternatives) => {
                for (index, alternative) in alternatives.iter().enumerate() {
                    self.pattern(alternative, &id, "alternatives", index as u64);
                }
            }
            PatternKind::Wildcard => {}
            PatternKind::Literal(value) => self.expr(value, &id, "value", 0),
            PatternKind::Range { lo, hi, .. } => {
                self.expr(lo, &id, "low", 0);
                self.expr(hi, &id, "high", 0);
            }
            PatternKind::Binding(name) => self.ident(*name, &id, "name", 0),
            PatternKind::Typed { name, ty } => {
                self.ident(*name, &id, "name", 0);
                self.ty(ty, &id, "type", 0);
            }
            PatternKind::Constructor { name, args } => {
                self.ident(*name, &id, "name", 0);
                for (index, argument) in args.iter().enumerate() {
                    self.pattern(argument, &id, "arguments", index as u64);
                }
            }
            PatternKind::List(elements) => {
                for (index, element) in elements.iter().enumerate() {
                    match element {
                        ListPatternElem::Pattern(element) => {
                            self.pattern(element, &id, "elements", index as u64);
                        }
                        ListPatternElem::Rest(binding) => {
                            let rest_span = binding
                                .as_ref()
                                .map_or(pattern.span, |binding| binding.span);
                            let rest_id = self.push(
                                "list-pattern/rest",
                                rest_span,
                                Some(&id),
                                "elements",
                                index as u64,
                                [],
                            );
                            if let Some(binding) = binding {
                                self.pattern(binding, &rest_id, "binding", 0);
                            }
                        }
                    }
                }
            }
            PatternKind::Record(fields) => self.pattern_fields(fields, &id),
            PatternKind::NominalRecord { name, fields } => {
                self.ident(*name, &id, "name", 0);
                self.pattern_fields(fields, &id);
            }
        }
    }

    fn pattern_fields(&mut self, fields: &[RecordPatternField], parent: &str) {
        for (index, field) in fields.iter().enumerate() {
            let id = self.push(
                "record-pattern-field",
                field.span,
                Some(parent),
                "fields",
                index as u64,
                [],
            );
            self.ident(field.name, &id, "name", 0);
            if let Some(pattern) = &field.pattern {
                self.pattern(pattern, &id, "pattern", 0);
            }
        }
    }

    fn ty(&mut self, ty: &Type, parent: &str, field: &str, index: u64) {
        let kind = match &ty.kind {
            TypeKind::Named { .. } => "type/named",
            TypeKind::Qualified { .. } => "type/qualified",
            TypeKind::Literal => "type/literal",
            TypeKind::Record(_) => "type/record",
            TypeKind::Function { .. } => "type/function",
            TypeKind::Unit => "type/unit",
            TypeKind::Union(_) => "type/union",
        };
        let id = self.push(kind, ty.span, Some(parent), field, index, []);
        match &ty.kind {
            TypeKind::Named { name, args } => {
                self.ident(*name, &id, "name", 0);
                for (index, argument) in args.iter().enumerate() {
                    self.ty(argument, &id, "arguments", index as u64);
                }
            }
            TypeKind::Qualified { ns, name, args } => {
                self.ident(*ns, &id, "namespace", 0);
                self.ident(*name, &id, "name", 0);
                for (index, argument) in args.iter().enumerate() {
                    self.ty(argument, &id, "arguments", index as u64);
                }
            }
            TypeKind::Literal | TypeKind::Unit => {}
            TypeKind::Record(fields) => {
                for (index, field) in fields.iter().enumerate() {
                    let field_id = self.push(
                        "record-type-field",
                        field.span,
                        Some(&id),
                        "fields",
                        index as u64,
                        [],
                    );
                    self.ident(field.name, &field_id, "name", 0);
                    self.ty(&field.ty, &field_id, "type", 0);
                }
            }
            TypeKind::Function { params, ret } => {
                for (index, parameter) in params.iter().enumerate() {
                    let param_id = self.push(
                        "function-type-parameter",
                        parameter.ty.span,
                        Some(&id),
                        "parameters",
                        index as u64,
                        [("variadic", boolean(parameter.variadic))],
                    );
                    self.ty(&parameter.ty, &param_id, "type", 0);
                }
                self.ty(ret, &id, "returnType", 0);
            }
            TypeKind::Union(members) => {
                for (index, member) in members.iter().enumerate() {
                    self.ty(member, &id, "members", index as u64);
                }
            }
        }
    }
}

pub(crate) fn front_end_counts(unit: &KernelUnit) -> (u64, u64, u64) {
    let raw = unit
        .resolved
        .modules
        .iter()
        .map(|module| module.raw_tokens.len() as u64)
        .sum();
    let layout = unit
        .resolved
        .modules
        .iter()
        .map(|module| module.layout_tokens.len() as u64)
        .sum();
    let ast = unit
        .resolved
        .modules
        .iter()
        .map(|module| {
            let source = unit.resolved.map.file(module.file).src();
            let identity = source_id(&module.identity, &module.path);
            let mut projector = AstProjector::counting(&identity, source);
            projector.program(&module.program);
            projector.next
        })
        .sum();
    (raw, layout, ast)
}

fn assign_op(op: AssignOp) -> &'static str {
    match op {
        AssignOp::Assign => "assign",
        AssignOp::Add => "add",
        AssignOp::Sub => "subtract",
        AssignOp::Mul => "multiply",
        AssignOp::Div => "divide",
        AssignOp::Rem => "remainder",
        AssignOp::Coalesce => "coalesce",
    }
}

fn unary_op(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Plus => "plus",
        UnaryOp::Minus => "minus",
        UnaryOp::Not => "not",
    }
}

fn binary_op(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Pow => "power",
        BinaryOp::Mul => "multiply",
        BinaryOp::Div => "divide",
        BinaryOp::Rem => "remainder",
        BinaryOp::Add => "add",
        BinaryOp::Sub => "subtract",
        BinaryOp::Lt => "less-than",
        BinaryOp::Le => "less-or-equal",
        BinaryOp::Gt => "greater-than",
        BinaryOp::Ge => "greater-or-equal",
        BinaryOp::Eq => "equal",
        BinaryOp::Ne => "not-equal",
        BinaryOp::In => "in",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
        BinaryOp::Coalesce => "coalesce",
    }
}

fn comp_kind(kind: CompKind) -> &'static str {
    match kind {
        CompKind::Array => "array",
        CompKind::Set => "set",
        CompKind::Map => "map",
    }
}
