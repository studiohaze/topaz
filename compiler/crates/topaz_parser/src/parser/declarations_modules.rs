use super::*;

impl Parser<'_> {
    // ---- v5.2 module items (SPEC §17, ADR-076 envelope) ---------------

    /// The bounded module-head decision (ADR-076): an identifier in
    /// the closed set `{import, export, use}` at `Program` item
    /// start, classified by its no-`Sep` follow token. `None` means
    /// the base reading wins (base priority). Tagged-template
    /// adjacency (`import"x"`) never reaches here -- it lexes as a
    /// tagged template, keeping its base diagnostic.
    pub(super) fn module_head(&self) -> Option<ModuleHead> {
        if self.version < LangVersion::V5_2 || self.peek() != TokenKind::Ident {
            return None;
        }
        let head = self.text(self.cur_span());
        let follow = self.peek_at(1);
        match head {
            "import" => match follow {
                TokenKind::Ident => Some(ModuleHead::Import),
                TokenKind::StringStart { .. } => Some(ModuleHead::ReservedPath),
                _ => None,
            },
            "export" => match follow {
                TokenKind::Kw(
                    Keyword::Function | Keyword::Type | Keyword::Let | Keyword::Const,
                ) => Some(ModuleHead::Export),
                TokenKind::Ident
                    if self.is_enum_head_at(1)
                        || self.is_record_head_at(1)
                        || self.is_newtype_head_at(1) =>
                {
                    Some(ModuleHead::Export)
                }
                TokenKind::LBrace => Some(ModuleHead::ExportList),
                TokenKind::Ident if self.text_at(1) == "import" => Some(ModuleHead::ExportImport),
                _ => None,
            },
            "use" => (follow == TokenKind::Ident).then_some(ModuleHead::ReservedUse),
            _ => None,
        }
    }

    /// Source text of the token `offset` positions ahead.
    pub(super) fn text_at(&self, offset: usize) -> &str {
        let span = self
            .tokens
            .get(self.pos + offset)
            .map_or_else(|| self.cur_span(), |t| t.span);
        self.text(span)
    }

    pub(super) fn module_item(&mut self, head: ModuleHead, prologue_done: bool) -> PResult<Stmt> {
        let lo = self.cur_span().lo;
        match head {
            ModuleHead::Import => {
                let item = self.import_item()?;
                if prologue_done {
                    self.error(
                        codes::IMPORT_PROLOGUE,
                        item.span,
                        "imports form a prologue: every `import` precedes all other top-level items",
                    );
                }
                Ok(Stmt {
                    kind: StmtKind::Import(item),
                    span: self.span_from(lo),
                })
            }
            ModuleHead::Export => self.export_item(),
            ModuleHead::ExportList => {
                let span = self.cur_span();
                self.error(
                    codes::REJECTED_MODULE_FORM,
                    span,
                    "export lists are invalid syntax; put `export` on each public declaration",
                );
                self.bump(); // export
                self.skip_balanced_braces();
                Err(Abort)
            }
            ModuleHead::ExportImport => {
                let span = self.cur_span();
                self.error(
                    codes::REJECTED_MODULE_FORM,
                    span,
                    "re-export syntax does not exist (`export import ...` is rejected)",
                );
                Err(Abort)
            }
            ModuleHead::ReservedPath => {
                let span = self.cur_span();
                self.error(
                    codes::RESERVED_MODULE_FORM,
                    span,
                    "string and template module paths are reserved and unused; module paths are dotted identifiers",
                );
                Err(Abort)
            }
            ModuleHead::ReservedUse => {
                let span = self.cur_span();
                self.error(
                    codes::RESERVED_MODULE_FORM,
                    span,
                    "`use` is reserved and unused; did you mean `import`?",
                );
                Err(Abort)
            }
        }
    }

    pub(super) fn import_item(&mut self) -> PResult<ImportItem> {
        let lo = self.cur_span().lo;
        self.bump(); // import
        let plo = self.cur_span().lo;
        let mut segments = vec![self.ident("a module path segment")?];
        while self.eat(TokenKind::Dot) {
            segments.push(self.ident("a module path segment")?);
        }
        let path = ModulePath {
            segments,
            span: self.span_from(plo),
        };
        let kind = if self.at(TokenKind::LBrace) {
            ImportKind::Selected {
                specs: self.import_list()?,
            }
        } else if self.at(TokenKind::Ident) && self.text(self.cur_span()) == "as" {
            self.bump(); // as
            let alias = self.ident("an import alias")?;
            if self.at(TokenKind::LBrace) {
                let span = self.cur_span();
                self.error(
                    codes::REJECTED_MODULE_FORM,
                    span,
                    "a namespace alias and a selection list do not compose (`import m as ns { x }` is invalid syntax)",
                );
                self.skip_balanced_braces();
            }
            ImportKind::Namespace { alias: Some(alias) }
        } else {
            ImportKind::Namespace { alias: None }
        };
        Ok(ImportItem {
            path,
            kind,
            span: self.span_from(lo),
        })
    }

    pub(super) fn import_list(&mut self) -> PResult<Vec<ImportSpec>> {
        let open = self.expect(TokenKind::LBrace, "`{`")?;
        let mut specs = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let lo = self.cur_span().lo;
            // `ImportSpec ::= Identifier ImportSpecAlias?` (SPEC v5.2
            // §17): keyword entries are not import names.
            if matches!(self.peek(), TokenKind::Kw(_)) {
                let span = self.cur_span();
                self.error(
                    codes::IMPORT_LIST_FORM,
                    span,
                    "an import name is an identifier; keywords cannot be selected",
                );
                return Err(Abort);
            }
            let name = self.ident("an import name")?;
            let alias = if self.at(TokenKind::Ident) && self.text(self.cur_span()) == "as" {
                self.bump(); // as
                Some(self.ident("an import alias")?)
            } else {
                None
            };
            specs.push(ImportSpec {
                name,
                alias,
                span: self.span_from(lo),
            });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        let close = self.expect(TokenKind::RBrace, "`}`")?;
        if specs.is_empty() {
            self.error(
                codes::IMPORT_LIST_FORM,
                Span::new(self.file, open.span.lo, close.span.hi),
                "a selection list selects at least one exported name",
            );
        }
        // Duplicate axes (SPEC v5.2 §17): the same exported source
        // name may not be selected twice; the same bound local name
        // may not be produced twice. Diagnose each later offending
        // spec once in source order, with source-name precedence.
        let mut source_names = HashSet::new();
        let mut bound_names = HashSet::new();
        let mut dups: Vec<(Span, &'static str)> = Vec::new();
        for spec in &specs {
            let source_name = self.text(spec.name.span);
            let bound_span = spec.alias.unwrap_or(spec.name).span;
            let bound_name = self.text(bound_span);
            let source_duplicate = !source_names.insert(source_name);
            let bound_duplicate = !bound_names.insert(bound_name);
            if source_duplicate {
                dups.push((
                    spec.name.span,
                    "this exported name is already selected in this list",
                ));
            } else if bound_duplicate {
                dups.push((bound_span, "this local name is already bound by this list"));
            }
        }
        for (span, message) in dups {
            self.error(codes::IMPORT_LIST_FORM, span, message);
        }
        Ok(specs)
    }

    pub(super) fn export_item(&mut self) -> PResult<Stmt> {
        let lo = self.cur_span().lo;
        self.bump(); // export
        let ilo = self.cur_span().lo;
        let kind = match self.peek() {
            TokenKind::Kw(Keyword::Function) => StmtKind::Function(self.function_decl()?),
            TokenKind::Kw(Keyword::Type) => StmtKind::TypeAlias(Rc::new(self.type_alias()?)),
            TokenKind::Ident if self.is_enum_head() => StmtKind::Enum(Rc::new(self.enum_decl()?)),
            TokenKind::Ident if self.is_record_head() => {
                StmtKind::Record(Rc::new(self.record_decl()?))
            }
            TokenKind::Ident if self.is_newtype_head() => {
                StmtKind::Newtype(Rc::new(self.newtype_decl()?))
            }
            TokenKind::Kw(Keyword::Let) => {
                let kind = self.let_binding()?;
                // `ExportLetBinding ::= "let" Identifier ...` (SPEC
                // v5.2 §17): exactly one identifier. `export let mut`
                // parses; its rejection is static semantics
                // (resolver-era).
                if let StmtKind::Let { pattern, .. } = &kind
                    && !matches!(
                        pattern.kind,
                        PatternKind::Binding(_) | PatternKind::Typed { .. }
                    )
                {
                    self.error(
                        codes::EXPORT_BINDING_FORM,
                        pattern.span,
                        "an exported `let` binds exactly one identifier; destructure privately, then export the identifier",
                    );
                }
                kind
            }
            TokenKind::Kw(Keyword::Const) => self.const_binding()?,
            _ => unreachable!("module_head guarantees a declaration head"),
        };
        let inner = Stmt {
            kind,
            span: self.span_from(ilo),
        };
        Ok(Stmt {
            kind: StmtKind::Export(Rc::new(inner)),
            span: self.span_from(lo),
        })
    }

    /// Error recovery: consumes a balanced `{ ... }` run.
    pub(super) fn skip_balanced_braces(&mut self) {
        if !self.at(TokenKind::LBrace) {
            return;
        }
        let mut depth = 0usize;
        while !self.at(TokenKind::Eof) {
            match self.peek() {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        self.bump();
                        return;
                    }
                }
                _ => {}
            }
            self.bump();
        }
    }

    // ---- program and statements ---------------------------------------

    pub(super) fn program(&mut self) -> Program {
        let mut items = Vec::new();
        let mut prologue_done = false;
        self.skip_seps();
        while !self.at(TokenKind::Eof) {
            let before = self.pos;
            // v5.2 module head words (SPEC §17, ADR-076): contextual
            // recognition at Program item start only, decided by the
            // bounded no-`Sep` follow table; base priority otherwise.
            if let Some(head) = self.module_head() {
                match self.module_item(head, prologue_done) {
                    Ok(stmt) => {
                        if !matches!(stmt.kind, StmtKind::Import(_)) {
                            prologue_done = true;
                        }
                        items.push(stmt);
                        self.item_boundary();
                    }
                    Err(Abort) => self.synchronize(),
                }
                if self.pos == before && !self.at(TokenKind::Eof) {
                    let span = self.cur_span();
                    self.error(codes::UNEXPECTED_TOKEN, span, "expected a statement");
                    self.bump();
                }
                self.skip_seps();
                continue;
            }
            match self.statement() {
                Ok(stmt) => {
                    prologue_done = true;
                    items.push(stmt);
                    self.item_boundary();
                }
                Err(Abort) => self.synchronize(),
            }
            if self.pos == before && !self.at(TokenKind::Eof) {
                // A stray closer or similar: force progress.
                let span = self.cur_span();
                self.error(codes::UNEXPECTED_TOKEN, span, "expected a statement");
                self.bump();
            }
            self.skip_seps();
        }
        Program {
            items,
            span: Span::new(self.file, 0, self.src.len() as u32),
        }
    }

    pub(super) fn statement(&mut self) -> PResult<Stmt> {
        let lo = self.cur_span().lo;
        let kind = match self.peek() {
            TokenKind::Kw(Keyword::Function) => StmtKind::Function(self.function_decl()?),
            TokenKind::Kw(Keyword::Type) => StmtKind::TypeAlias(Rc::new(self.type_alias()?)),
            TokenKind::Kw(Keyword::Let) => self.let_binding()?,
            TokenKind::Kw(Keyword::Const) => self.const_binding()?,
            TokenKind::Kw(Keyword::Mut) => {
                // `mut` is only valid IMMEDIATELY after `let`. At statement start
                // (`mut let x = 1`, or a bare `mut`) it is a misplaced `let mut`;
                // guide rather than report a bare "expected an expression".
                return Err(
                    self.error_here("`mut` comes right after `let` — write `let mut <name> = …`")
                );
            }
            TokenKind::Kw(Keyword::Return) => {
                self.bump();
                let value = if matches!(
                    self.peek(),
                    TokenKind::Sep | TokenKind::RBrace | TokenKind::Eof
                ) {
                    None
                } else {
                    Some(self.expr()?)
                };
                StmtKind::Return(value)
            }
            TokenKind::Kw(Keyword::Defer) => {
                self.bump();
                let body = self.expr()?;
                if !matches!(body.kind, ExprKind::Block(_) | ExprKind::Call { .. }) {
                    self.error(
                        codes::INVALID_DEFER_BODY,
                        body.span,
                        "`defer` takes a block or a call",
                    );
                }
                StmtKind::Defer(Rc::new(body))
            }
            TokenKind::Ident if self.is_using_head() => self.using_block()?,
            TokenKind::Kw(Keyword::While) => {
                let wlo = self.cur_span().lo;
                self.bump();
                // v5.4 `while let <pattern> = <expr> { … }` — DESUGARED to a
                // `while true { match … }` (see `while_let`). Gated `>= V5_4`; at
                // older editions `let` after `while` falls through to `self.expr()`
                // and is reported as "expected an expression".
                if self.version >= LangVersion::V5_4 && self.at(TokenKind::Kw(Keyword::Let)) {
                    self.while_let(wlo)?
                } else {
                    let cond = self.expr_before_block()?;
                    let body = self.block()?;
                    StmtKind::While {
                        cond: Rc::new(cond),
                        body: Rc::new(body),
                    }
                }
            }
            TokenKind::Kw(Keyword::Break) => {
                self.bump();
                // An optional `'label` then an optional value expression.
                // `break <value>` yields the value as the target loop's result; a
                // bare `break` yields Unit. The value is present unless the next
                // token closes the statement (mirrors `return`). Labels/values are
                // gated `>= V5_4` (at older editions a `'label` never lexes — `'`
                // is an UNKNOWN_CHAR — and a value falls through to "an expression").
                let label = self.opt_loop_label();
                let value = if self.version >= LangVersion::V5_4
                    && !matches!(
                        self.peek(),
                        TokenKind::Sep | TokenKind::RBrace | TokenKind::Eof
                    ) {
                    Some(self.expr()?)
                } else {
                    None
                };
                StmtKind::Break { label, value }
            }
            TokenKind::Kw(Keyword::Continue) => {
                self.bump();
                // An optional `'label` (value-less). `continue` jumps to
                // the next iteration of the target loop.
                let label = self.opt_loop_label();
                StmtKind::Continue { label }
            }
            // v5.3 `enum Name { … }` — contextual (see `is_enum_head`); falls
            // through to expr/assignment when `enum` is an ordinary identifier.
            TokenKind::Ident if self.is_enum_head() => StmtKind::Enum(Rc::new(self.enum_decl()?)),
            // v5.4 `record Name { … }` — contextual (see `is_record_head`); falls
            // through to expr/assignment when `record` is an ordinary identifier.
            TokenKind::Ident if self.is_record_head() => {
                StmtKind::Record(Rc::new(self.record_decl()?))
            }
            // v5.4 `newtype Name = T` — contextual (see `is_newtype_head`); falls
            // through to expr/assignment when `newtype` is an ordinary identifier.
            TokenKind::Ident if self.is_newtype_head() => {
                StmtKind::Newtype(Rc::new(self.newtype_decl()?))
            }
            // v5.4 `impl Name { … }` / `impl Proto<Type> { … }` — contextual (see
            // `is_impl_head`); falls through to expr/assignment when `impl` is an
            // ordinary identifier.
            TokenKind::Ident if self.is_impl_head() => StmtKind::Impl(self.impl_decl()?),
            // v5.4 `protocol Name { … }` / `protocol Name<T> { … }` — contextual (see
            // `is_protocol_head`); falls through when `protocol` is an ordinary ident.
            TokenKind::Ident if self.is_protocol_head() => {
                StmtKind::Protocol(self.protocol_decl()?)
            }
            _ => self.expr_or_assignment()?,
        };
        Ok(Stmt {
            kind,
            span: self.span_from(lo),
        })
    }

    pub(super) fn expr_or_assignment(&mut self) -> PResult<StmtKind> {
        let target = self.expr()?;
        let op = match self.peek() {
            TokenKind::Eq => AssignOp::Assign,
            TokenKind::PlusEq => AssignOp::Add,
            TokenKind::MinusEq => AssignOp::Sub,
            TokenKind::StarEq => AssignOp::Mul,
            TokenKind::SlashEq => AssignOp::Div,
            TokenKind::PercentEq => AssignOp::Rem,
            TokenKind::QuestionQuestionEq => AssignOp::Coalesce,
            _ => return Ok(StmtKind::Expr(target)),
        };
        self.bump();
        if !matches!(
            target.kind,
            ExprKind::Ident | ExprKind::Member { .. } | ExprKind::Index { .. }
        ) {
            self.error(
                codes::INVALID_ASSIGNMENT_TARGET,
                target.span,
                "assignment target must be an identifier, member access, or index access",
            );
        }
        let value = self.expr()?;
        Ok(StmtKind::Assign {
            target: Rc::new(target),
            op,
            value: Rc::new(value),
        })
    }

    pub(super) fn is_using_head(&self) -> bool {
        self.version >= LangVersion::V5_4
            && self.peek() == TokenKind::Ident
            && self.text_at(0) == "using"
            && self.peek_at(1) == TokenKind::Ident
            && self.peek_at(2) == TokenKind::Eq
    }

    pub(super) fn using_block(&mut self) -> PResult<StmtKind> {
        self.bump(); // using
        let name = self.binding_ident("a resource binding name")?;
        self.expect(TokenKind::Eq, "`=`")?;
        let value = self.expr_before_block()?;
        let body = self.block()?;
        Ok(StmtKind::Using {
            name,
            value,
            body: Rc::new(body),
        })
    }

    pub(super) fn let_binding(&mut self) -> PResult<StmtKind> {
        self.bump(); // let
        let mutable = self.eat(TokenKind::Kw(Keyword::Mut));
        let pattern = if mutable {
            // `let mut` binds an identifier, not a general pattern.
            let name = self.binding_ident("a binding name")?;
            Pattern {
                span: name.span,
                kind: PatternKind::Binding(name),
            }
        } else {
            self.pattern()?
        };
        let ty = if self.eat(TokenKind::Colon) {
            Some(self.type_()?)
        } else {
            None
        };
        // `let x mut = …` — the `mut` is in the wrong place; it belongs right after
        // `let`. Catch it before the bare "expected `=`" so the message guides.
        if !mutable && self.at(TokenKind::Kw(Keyword::Mut)) {
            return Err(
                self.error_here("`mut` comes right after `let` — write `let mut <name> = …`")
            );
        }
        self.expect(TokenKind::Eq, "`=`")?;
        let value = self.expr()?;
        Ok(StmtKind::Let {
            mutable,
            pattern: Rc::new(pattern),
            ty,
            value,
        })
    }

    pub(super) fn const_binding(&mut self) -> PResult<StmtKind> {
        self.bump(); // const
        let name = self.binding_ident("a constant name")?;
        let ty = if self.eat(TokenKind::Colon) {
            Some(self.type_()?)
        } else {
            None
        };
        self.expect(TokenKind::Eq, "`=`")?;
        let value = self.expr()?;
        Ok(StmtKind::Const { name, ty, value })
    }

    pub(super) fn function_decl(&mut self) -> PResult<FunctionDecl> {
        self.bump(); // function
        let name = self.ident("a function name")?;
        let (type_params, type_param_bounds) = self.function_type_params()?;
        self.expect(TokenKind::LParen, "`(`")?;
        let mut params = Vec::new();
        let mut first = true;
        while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
            let lo = self.cur_span().lo;
            let variadic = self.eat(TokenKind::Ellipsis);
            let name = self.binding_ident("a parameter name")?;
            // v5.4 receiver methods: a bare `self` FIRST parameter (no `: Type`) is
            // the method receiver. Its type is the enclosing `impl` type, resolved at
            // check time — here it carries a placeholder `Type` pointing at the
            // `self` span (never formed, since the checker skips the receiver slot).
            let self_receiver =
                first && !variadic && self.text(name.span) == "self" && !self.at(TokenKind::Colon);
            let ty = if self_receiver {
                Type {
                    kind: TypeKind::Named {
                        name,
                        args: Vec::new(),
                    },
                    span: name.span,
                }
            } else {
                self.expect(TokenKind::Colon, "`:`")?;
                self.type_()?
            };
            let default = if self.eat(TokenKind::Eq) {
                Some(self.expr()?)
            } else {
                None
            };
            first = false;
            params.push(Param {
                name,
                ty,
                default,
                variadic,
                span: self.span_from(lo),
            });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen, "`)`")?;
        let return_type = if self.eat(TokenKind::ThinArrow) {
            Some(self.type_()?)
        } else {
            None
        };
        let body = self.block()?;
        Ok(FunctionDecl {
            name,
            type_params,
            type_param_bounds,
            params,
            return_type,
            body: Rc::new(body),
        })
    }

    pub(super) fn type_alias(&mut self) -> PResult<TypeAlias> {
        self.bump(); // type
        let name = self.ident("a type name")?;
        let type_params = self.type_params()?;
        self.expect(TokenKind::Eq, "`=`")?;
        let ty = self.type_()?;
        Ok(TypeAlias {
            name,
            type_params,
            ty: Rc::new(ty),
        })
    }

    /// v5.3 user enums are recognized CONTEXTUALLY (`enum` is an ordinary
    /// identifier, not a reserved keyword — ADR-071), so a bare `enum`, `enum =
    /// x`, `enum(x)`, `enum.f` all stay identifier uses. Only the exact head
    /// `enum <Name> {` is an enum declaration. User enums begin at v5.3,
    /// so this is gated `>= V5_3` (NOT available at v5.1/v5.2, where
    /// `enum` stays an ordinary identifier).
    pub(super) fn is_enum_head(&self) -> bool {
        self.is_enum_head_at(0)
    }

    pub(super) fn is_enum_head_at(&self, offset: usize) -> bool {
        self.version >= LangVersion::V5_3
            && self.peek_at(offset) == TokenKind::Ident
            && self.text_at(offset) == "enum"
            && self.peek_at(offset + 1) == TokenKind::Ident
            // `enum Name {` OR (v5.4) `enum Name<T> {` / `enum Name derives …`.
            // The `derives` clause (§4) sits between the name/type-params and the
            // body, so the head also opens on a following contextual `derives`.
            && (self.peek_at(offset + 2) == TokenKind::LBrace
                || (self.version >= LangVersion::V5_4
                    && self.peek_at(offset + 2) == TokenKind::Lt)
                || (self.version >= LangVersion::V5_4
                    && self.peek_at(offset + 2) == TokenKind::Ident
                    && self.text_at(offset + 2) == "derives"))
    }

    /// `enum Name<T> { Variant, Variant(T1, …), … }` — closed nominal sum. Generic
    /// enum heads are v5.4-only; v5.3 still accepts only the non-generic head.
    pub(super) fn enum_decl(&mut self) -> PResult<EnumDecl> {
        self.bump(); // `enum` (an identifier token)
        let name = self.ident("an enum name")?;
        let type_params = if self.version >= LangVersion::V5_4 {
            self.type_params()?
        } else {
            Vec::new()
        };
        let derives = self.derives_clause()?;
        self.expect(TokenKind::LBrace, "`{`")?;
        let mut variants = Vec::new();
        while self.peek() != TokenKind::RBrace && self.peek() != TokenKind::Eof {
            let vname = self.ident("a variant name")?;
            let payload = if self.peek() == TokenKind::LParen {
                self.bump(); // `(`
                let mut tys = Vec::new();
                loop {
                    tys.push(self.type_()?);
                    if self.peek() == TokenKind::Comma {
                        self.bump();
                    } else {
                        break;
                    }
                }
                self.expect(TokenKind::RParen, "`)`")?;
                Some(tys)
            } else {
                None
            };
            let span = vname.span;
            variants.push(EnumVariant {
                name: vname,
                payload,
                span,
            });
            if self.peek() == TokenKind::Comma {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(TokenKind::RBrace, "`}`")?;
        Ok(EnumDecl {
            name,
            type_params,
            variants,
            derives,
        })
    }

    /// `derives Eq, Order, Show` — an OPTIONAL clause between a record/enum NAME and
    /// its body `{` (v5.4 §4): `record User derives Eq, Order, Show { … }`. `derives`
    /// is a CONTEXTUAL identifier (mirrors `is_impl_head`): only the exact token text
    /// `derives` in this position opens the clause, so `derives` remains a usable
    /// identifier everywhere else. Gated `>= V5_4`; at older editions the clause is
    /// never recognized. The names form a non-empty comma-separated identifier list;
    /// membership/derivability is the CHECKER's job (this only records the surface
    /// list).
    pub(super) fn derives_clause(&mut self) -> PResult<Vec<Ident>> {
        if !(self.version >= LangVersion::V5_4
            && self.peek() == TokenKind::Ident
            && self.text_at(0) == "derives")
        {
            return Ok(Vec::new());
        }
        self.bump(); // `derives` (an identifier token)
        let mut names = vec![self.ident("a protocol name to derive")?];
        while self.peek() == TokenKind::Comma {
            self.bump(); // `,`
            names.push(self.ident("a protocol name to derive")?);
        }
        Ok(names)
    }

    /// `record <Name> {` is a record declaration. Nominal records are a v5.4-only
    /// feature, so this is gated `>= V5_4` (NOT available at v5.1/v5.2/v5.3, where
    /// `record` stays an ordinary identifier).
    pub(super) fn is_record_head(&self) -> bool {
        self.is_record_head_at(0)
    }

    pub(super) fn is_record_head_at(&self, offset: usize) -> bool {
        self.version >= LangVersion::V5_4
            && self.peek_at(offset) == TokenKind::Ident
            && self.text_at(offset) == "record"
            && self.peek_at(offset + 1) == TokenKind::Ident
            // `record Name {` OR `record Name<T> {` OR `record Name derives …` —
            // the `derives` clause (§4) sits between the name/type-params and the body.
            && (self.peek_at(offset + 2) == TokenKind::LBrace
                || self.peek_at(offset + 2) == TokenKind::Lt
                || (self.peek_at(offset + 2) == TokenKind::Ident
                    && self.text_at(offset + 2) == "derives"))
    }

    /// `record Name<T> { field: T, field: T = default, … }` — nominal product.
    /// Each field is `name: Type` with an optional `= default` expression.
    pub(super) fn record_decl(&mut self) -> PResult<RecordDecl> {
        self.bump(); // `record` (an identifier token)
        let name = self.ident("a record name")?;
        let type_params = self.type_params()?;
        // §6 (v5.4) `map`/`set` are reserved as RECORD names: at `>= V5_4` the
        // contextual `map { … }` / `set { … }` literal recognition would shadow
        // brace-CONSTRUCTION of such a record (`map { x: 1 }` parses as a map
        // literal, never the record), so a record so named would be unconstructable.
        // Reject the DECLARATION with a clear message instead of a baffling
        // downstream error. (Other reserved-but-constructable names are unaffected.)
        if self.version >= LangVersion::V5_4 {
            let text = self.text(name.span).to_string();
            if text == "map" || text == "set" {
                self.error(
                    codes::RESERVED_BINDING_NAME,
                    name.span,
                    &format!(
                        "`{text}` is reserved for the `{text} {{ … }}` collection literal (§6) and cannot be a record name"
                    ),
                );
            }
        }
        let derives = self.derives_clause()?;
        self.expect(TokenKind::LBrace, "`{`")?;
        let mut fields = Vec::new();
        while self.peek() != TokenKind::RBrace && self.peek() != TokenKind::Eof {
            let lo = self.cur_span().lo;
            let fname = self.ident("a field name")?;
            self.expect(TokenKind::Colon, "`:`")?;
            let ty = self.type_()?;
            let default = if self.peek() == TokenKind::Eq {
                self.bump(); // `=`
                Some(Rc::new(self.expr()?))
            } else {
                None
            };
            fields.push(RecordFieldDecl {
                name: fname,
                ty,
                default,
                span: self.span_from(lo),
            });
            if self.peek() == TokenKind::Comma {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(TokenKind::RBrace, "`}`")?;
        Ok(RecordDecl {
            name,
            type_params,
            fields,
            derives,
        })
    }

    /// `newtype <Name> =` is a newtype declaration. Newtypes are a v5.4-only
    /// feature, so this is gated `>= V5_4` (NOT available at v5.1/v5.2/v5.3, where
    /// `newtype` stays an ordinary identifier). The `=` lookahead distinguishes it
    /// from a bare `newtype` identifier used in an expression.
    pub(super) fn is_newtype_head(&self) -> bool {
        self.is_newtype_head_at(0)
    }

    pub(super) fn is_newtype_head_at(&self, offset: usize) -> bool {
        self.version >= LangVersion::V5_4
            && self.peek_at(offset) == TokenKind::Ident
            && self.text_at(offset) == "newtype"
            && self.peek_at(offset + 1) == TokenKind::Ident
            && matches!(self.peek_at(offset + 2), TokenKind::Eq | TokenKind::Lt)
    }

    /// `newtype Name<T> = BaseType` — a distinct nominal wrapper. The base is any
    /// type expression.
    pub(super) fn newtype_decl(&mut self) -> PResult<NewtypeDecl> {
        self.bump(); // `newtype` (an identifier token)
        let name = self.ident("a newtype name")?;
        let type_params = self.type_params()?;
        self.expect(TokenKind::Eq, "`=`")?;
        let base = self.type_()?;
        Ok(NewtypeDecl {
            name,
            type_params,
            base,
        })
    }

    /// `impl <Name> {` (inherent) or `impl <Proto> <Type> {` (a v5.4 §4 PROTOCOL
    /// impl `impl Show<User> { … }`) is an impl block. Impls are a v5.4-only
    /// feature, so this is gated `>= V5_4` (NOT available at v5.1/v5.2/v5.3, where
    /// `impl` stays an ordinary identifier). The `Name {` / `Name <` lookahead
    /// distinguishes it from a bare `impl` identifier in an expression.
    pub(super) fn is_impl_head(&self) -> bool {
        self.version >= LangVersion::V5_4
            && self.peek() == TokenKind::Ident
            && self.text_at(0) == "impl"
            && self.peek_at(1) == TokenKind::Ident
            && matches!(self.peek_at(2), TokenKind::LBrace | TokenKind::Lt)
    }

    /// `impl Name { … }` — an INHERENT block of receiver methods on the nominal type
    /// `Name` (each method takes `self`).
    ///
    /// `impl Proto<Type> { … }` (v5.4 §4) — a MANUAL PROTOCOL impl: `Proto` is the
    /// protocol, `<Type>` the conforming type, and each method is a FREE function (no
    /// `self`). Both forms parse the same `function`-only body; the `self`-first
    /// requirement (inherent) vs free-function shape (protocol) is enforced at check
    /// time. MVP: a single conforming type argument (`impl Show<User>`), no generics.
    pub(super) fn impl_decl(&mut self) -> PResult<ImplDecl> {
        self.bump(); // `impl` (an identifier token)
        let name = self.ident("a type name")?;
        // §4 PROTOCOL impl: `impl Proto<Type>` — `name` is the protocol, the single
        // type argument is the conforming type. Inherent `impl Name {` has no `<…>`.
        let target = if self.eat(TokenKind::Lt) {
            let target = self.ident("a conforming type name")?;
            self.expect_close_angle()?;
            Some(target)
        } else {
            None
        };
        self.expect(TokenKind::LBrace, "`{`")?;
        let mut methods = Vec::new();
        self.skip_seps();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let mlo = self.cur_span().lo;
            let exported = self.peek() == TokenKind::Ident && self.text_at(0) == "export";
            if exported {
                self.bump(); // `export`
            }
            if !self.at(TokenKind::Kw(Keyword::Function)) {
                return Err(self.error_here("an impl body contains only `function` methods"));
            }
            let decl = self.function_decl()?; // consumes `function`
            methods.push(ImplMethod {
                exported,
                decl,
                span: self.span_from(mlo),
            });
            self.skip_seps();
        }
        self.expect(TokenKind::RBrace, "`}`")?;
        Ok(ImplDecl {
            name,
            target,
            methods,
        })
    }

    /// `protocol <Name> {` / `protocol <Name> <` is a protocol declaration (v5.4 §4).
    /// Gated `>= V5_4`; `protocol` stays an ordinary identifier at older editions.
    pub(super) fn is_protocol_head(&self) -> bool {
        self.version >= LangVersion::V5_4
            && self.peek() == TokenKind::Ident
            && self.text_at(0) == "protocol"
            && self.peek_at(1) == TokenKind::Ident
            && matches!(self.peek_at(2), TokenKind::LBrace | TokenKind::Lt)
    }

    /// `protocol Show { function show(value: Self) -> string … }` /
    /// `protocol Show<T> { function show(value: T) -> string }` (v5.4 §4) — a protocol
    /// declaration: a named set of free-function method SIGNATURES (no body — spec §4
    /// has no default methods). The conforming-type stand-in is `Self` (no type
    /// params) OR the declared `<T>`. A `FunctionDecl` carries each signature with an
    /// empty body block (never lowered — a protocol method has no implementation).
    pub(super) fn protocol_decl(&mut self) -> PResult<ProtocolDecl> {
        self.bump(); // `protocol` (an identifier token)
        let name = self.ident("a protocol name")?;
        let type_params = self.type_params()?;
        self.expect(TokenKind::LBrace, "`{`")?;
        let mut methods = Vec::new();
        self.skip_seps();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            if !self.at(TokenKind::Kw(Keyword::Function)) {
                return Err(self.error_here("a protocol body contains only `function` signatures"));
            }
            methods.push(self.protocol_method_sig()?);
            self.skip_seps();
        }
        self.expect(TokenKind::RBrace, "`}`")?;
        Ok(ProtocolDecl {
            name,
            type_params,
            methods,
        })
    }

    /// One protocol method SIGNATURE `function show(value: T) -> string` (v5.4 §4) —
    /// like `function_decl` but with NO body (a protocol method is unimplemented). The
    /// resulting `FunctionDecl` carries an empty placeholder body block that is never
    /// lowered.
    pub(super) fn protocol_method_sig(&mut self) -> PResult<FunctionDecl> {
        self.bump(); // `function`
        let name = self.ident("a function name")?;
        let (type_params, type_param_bounds) = self.function_type_params()?;
        self.expect(TokenKind::LParen, "`(`")?;
        let mut params = Vec::new();
        while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
            let lo = self.cur_span().lo;
            let variadic = self.eat(TokenKind::Ellipsis);
            let pname = self.binding_ident("a parameter name")?;
            self.expect(TokenKind::Colon, "`:`")?;
            let ty = self.type_()?;
            let default = if self.eat(TokenKind::Eq) {
                Some(self.expr()?)
            } else {
                None
            };
            params.push(Param {
                name: pname,
                ty,
                default,
                variadic,
                span: self.span_from(lo),
            });
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen, "`)`")?;
        let return_type = if self.eat(TokenKind::ThinArrow) {
            Some(self.type_()?)
        } else {
            None
        };
        // No body: a protocol method is unimplemented. Synthesize an empty block at
        // the current position so the `FunctionDecl` shape is uniform.
        let empty_span = self.cur_span();
        let body = Block {
            stmts: Vec::new(),
            tail: None,
            span: empty_span,
        };
        Ok(FunctionDecl {
            name,
            type_params,
            type_param_bounds,
            params,
            return_type,
            body: Rc::new(body),
        })
    }

    pub(super) fn function_type_params(&mut self) -> PResult<(Vec<Ident>, Vec<Vec<Ident>>)> {
        let mut params = Vec::new();
        let mut bounds = Vec::new();
        if self.eat(TokenKind::Lt) {
            loop {
                params.push(self.ident("a type parameter")?);
                let mut param_bounds = Vec::new();
                if self.at(TokenKind::Colon) {
                    let colon = self.cur_span();
                    self.bump();
                    if self.version < LangVersion::V5_4 {
                        self.error(
                            codes::UNEXPECTED_TOKEN,
                            colon,
                            "generic protocol bounds need v5.4",
                        );
                    }
                    loop {
                        param_bounds.push(self.ident("a protocol bound")?);
                        if !self.eat(TokenKind::Plus) {
                            break;
                        }
                    }
                }
                bounds.push(param_bounds);
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            self.expect_close_angle()?;
        }
        Ok((params, bounds))
    }

    pub(super) fn type_params(&mut self) -> PResult<Vec<Ident>> {
        let mut params = Vec::new();
        if self.eat(TokenKind::Lt) {
            loop {
                params.push(self.ident("a type parameter")?);
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
            self.expect_close_angle()?;
        }
        Ok(params)
    }

    pub(super) fn block(&mut self) -> PResult<Block> {
        let open = self.expect(TokenKind::LBrace, "`{`")?;
        // Braces re-allow expression forms whose restriction is top-level only.
        let saved_lambda = std::mem::replace(&mut self.naked_lambda_ok, true);
        let saved_record_update = std::mem::replace(&mut self.record_update_ok, true);
        let mut stmts = Vec::new();
        let mut tail = None;
        self.skip_seps();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let before = self.pos;
            match self.statement() {
                Ok(stmt) => {
                    if self.at(TokenKind::RBrace) {
                        // No separator before `}`: a trailing
                        // expression is the block value (SPEC §1a).
                        if let StmtKind::Expr(e) = stmt.kind {
                            tail = Some(Rc::new(e));
                        } else {
                            stmts.push(stmt);
                        }
                        break;
                    }
                    stmts.push(stmt);
                    self.item_boundary();
                }
                Err(Abort) => self.synchronize(),
            }
            if self.pos == before && !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
                let span = self.cur_span();
                self.error(codes::UNEXPECTED_TOKEN, span, "expected a statement");
                self.bump();
            }
            self.skip_seps();
        }
        self.naked_lambda_ok = saved_lambda;
        self.record_update_ok = saved_record_update;
        let close = self.expect(TokenKind::RBrace, "`}`")?;
        Ok(Block {
            stmts,
            tail,
            span: Span::new(self.file, open.span.lo, close.span.hi),
        })
    }
}
