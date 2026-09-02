//! Current checked Topaz to backend-neutral structured ANF.
//!
//! This crate is the only current-mode boundary allowed to inspect resolved
//! syntax. Backends consume [`topaz_hir::LoweredUnit`], never parser or resolver
//! models.

use std::collections::{BTreeMap, BTreeSet};

use topaz_check::CheckedUnit;
use topaz_diag::Span;
use topaz_hir::{
    LoweredBinding, LoweredControl, LoweredControlKind, LoweredExpressionKind, LoweredModule,
    LoweredOperation, LoweredOperationKind, LoweredPatternKind, LoweredRole, LoweredStorage,
    LoweredUnit, RuntimeRegistry, SemanticType, TypedNodeKind,
};
use topaz_resolve::{ResolveOutput, ResolvedReferenceFact};
use topaz_syntax::ast;

mod emission;

pub fn lower_checked(
    resolved: &ResolveOutput,
    checked: &CheckedUnit,
) -> Result<LoweredUnit, LowerError> {
    if resolved
        .diagnostics
        .iter()
        .chain(&checked.diagnostics)
        .any(|diagnostic| diagnostic.severity == topaz_diag::Severity::Error)
    {
        return Err(LowerError::DiagnosticsPresent);
    }
    let typed = checked
        .typed_hir
        .as_ref()
        .ok_or(LowerError::MissingTypedUnit)?;
    let calls = enriched_calls(resolved, &typed.calls);
    let captures = derive_resolution_captures(resolved, typed);
    let mut builder = Builder {
        resolved,
        semantic_types: typed
            .nodes
            .iter()
            .map(|node| {
                (
                    (node.span.file.0, node.span.lo, node.span.hi, node.kind),
                    &node.ty,
                )
            })
            .collect(),
        representations: typed
            .locals
            .iter()
            .map(|local| {
                (
                    (local.span.file.0, local.span.lo, local.span.hi),
                    local.mono,
                )
            })
            .collect(),
        references: resolved_reference_index(resolved),
        operations: Vec::new(),
        module_operations: BTreeMap::new(),
        closure_operations: BTreeMap::new(),
        calls: calls
            .iter()
            .map(|call| ((call.span.file.0, call.span.lo, call.span.hi), call))
            .collect(),
    };
    for module in &resolved.modules {
        builder.lower_module(module);
    }
    for capture in &captures {
        builder.lower_capture(capture)?;
    }
    sort_operations(resolved, &mut builder.operations);
    let mut captures_by_module = BTreeMap::<&str, Vec<&topaz_hir::TypedCapture>>::new();
    for capture in &captures {
        captures_by_module
            .entry(capture.module.as_str())
            .or_default()
            .push(capture);
    }
    let modules = resolved
        .modules
        .iter()
        .enumerate()
        .map(|(ordinal, module)| {
            let source = resolved.map.file(module.file).src();
            let (program, mut text) = emission::program(&module.identity, &module.program, source);
            text.mark_checked();
            if let Some(module_captures) = captures_by_module.get(module.identity.as_str()) {
                for capture in module_captures {
                    text.insert_capture(capture.closure_span, capture.name.clone());
                }
            }
            LoweredModule {
                identity: module.identity.clone(),
                path: module.path.clone(),
                file: module.file,
                initialization_ordinal: ordinal as u32,
                is_entry: module.is_entry,
                is_extern: module.is_extern,
                is_generated_std: module.is_generated_std,
                extern_replay_error: module.extern_replay_error.clone(),
                program,
                text,
                operation_ids: builder
                    .module_operations
                    .remove(&module.identity)
                    .unwrap_or_default(),
            }
        })
        .collect();
    let runtime = RuntimeRegistry::for_operations(&builder.operations);
    Ok(LoweredUnit {
        language_version: resolved.language_version,
        modules,
        operations: builder.operations,
        calls,
        captures,
        typed: Some(typed.clone()),
        import_edges: resolved.import_edges.clone(),
        runtime,
    })
}

/// Named compatibility lowering for explicit unchecked/older-mode routes and
/// emitter unit tests. It produces the same source-free emission model but
/// carries no checker-owned semantic facts and is never valid self-hosting
/// evidence.
pub fn lower_resolved_compat(resolved: &ResolveOutput) -> Result<LoweredUnit, LowerError> {
    let mut builder = Builder {
        resolved,
        semantic_types: BTreeMap::new(),
        representations: BTreeMap::new(),
        references: resolved_reference_index(resolved),
        operations: Vec::new(),
        module_operations: BTreeMap::new(),
        closure_operations: BTreeMap::new(),
        calls: BTreeMap::new(),
    };
    for module in &resolved.modules {
        builder.lower_module(module);
    }
    sort_operations(resolved, &mut builder.operations);
    let modules = resolved
        .modules
        .iter()
        .enumerate()
        .map(|(ordinal, module)| {
            let source = resolved.map.file(module.file).src();
            let (program, text) = emission::program(&module.identity, &module.program, source);
            LoweredModule {
                identity: module.identity.clone(),
                path: module.path.clone(),
                file: module.file,
                initialization_ordinal: ordinal as u32,
                is_entry: module.is_entry,
                is_extern: module.is_extern,
                is_generated_std: module.is_generated_std,
                extern_replay_error: module.extern_replay_error.clone(),
                program,
                text,
                operation_ids: builder
                    .module_operations
                    .remove(&module.identity)
                    .unwrap_or_default(),
            }
        })
        .collect();
    let runtime = RuntimeRegistry::for_operations(&builder.operations);
    Ok(LoweredUnit {
        language_version: resolved.language_version,
        modules,
        operations: builder.operations,
        calls: Vec::new(),
        captures: Vec::new(),
        typed: None,
        import_edges: resolved.import_edges.clone(),
        runtime,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerError {
    DiagnosticsPresent,
    MissingTypedUnit,
    UnknownCaptureParent {
        module: String,
        name: String,
        lo: u32,
        hi: u32,
    },
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DiagnosticsPresent => formatter.write_str("cannot lower a rejected unit"),
            Self::MissingTypedUnit => formatter.write_str("clean unit has no Typed IR"),
            Self::UnknownCaptureParent {
                module,
                name,
                lo,
                hi,
            } => write!(
                formatter,
                "capture `{name}` in module `{module}` has no lowered closure parent at {lo}..{hi}"
            ),
        }
    }
}

impl std::error::Error for LowerError {}

struct Builder<'a> {
    resolved: &'a ResolveOutput,
    semantic_types: BTreeMap<(u32, u32, u32, TypedNodeKind), &'a SemanticType>,
    representations: BTreeMap<(u32, u32, u32), topaz_hir::MonoTy>,
    references: BTreeMap<(u32, u32, u32), &'a ResolvedReferenceFact>,
    operations: Vec<LoweredOperation>,
    module_operations: BTreeMap<String, Vec<String>>,
    closure_operations: BTreeMap<(u32, u32, u32), usize>,
    calls: BTreeMap<(u32, u32, u32), &'a topaz_hir::TypedCall>,
}

impl Builder<'_> {
    fn lower_module(&mut self, module: &topaz_resolve::ResolvedModule) {
        let module_id = stable_id(
            &module.identity,
            LoweredRole::ModuleInitialization,
            "module",
            module.program.span,
        );
        let operands = module
            .program
            .items
            .iter()
            .map(|statement| self.statement(module, statement, Some(&module_id)))
            .collect();
        self.push(
            &module.identity,
            LoweredOperation {
                id: module_id,
                module: module.identity.clone(),
                span: module.program.span,
                parent: None,
                role: LoweredRole::ModuleInitialization,
                kind: LoweredOperationKind::Module,
                operands,
                semantic_type: None,
                representation: None,
                binding: None,
                control: None,
                call: None,
                runtime_leaf: None,
            },
        );
    }

    fn statement(
        &mut self,
        module: &topaz_resolve::ResolvedModule,
        statement: &ast::Stmt,
        parent: Option<&str>,
    ) -> String {
        use ast::StmtKind as S;
        let tag = match &statement.kind {
            S::Import(_) => "import",
            S::Export(_) => "export",
            S::Function(_) => "function",
            S::TypeAlias(_) => "type-alias",
            S::Enum(_) => "enum",
            S::Record(_) => "record",
            S::Newtype(_) => "newtype",
            S::Impl(_) => "implementation",
            S::Protocol(_) => "protocol",
            S::Let { .. } => "let",
            S::Const { .. } => "constant",
            S::Assign { .. } => "assignment",
            S::Return(_) => "return",
            S::Defer(_) => "defer",
            S::Using { .. } => "using",
            S::While { .. } => "while",
            S::Break { .. } => "break",
            S::Continue { .. } => "continue",
            S::Expr(_) => "expression-statement",
        };
        let id = stable_id(
            &module.identity,
            LoweredRole::Statement,
            tag,
            statement.span,
        );
        let mut binding = None;
        let mut control = None;
        let mut closure_span = None;
        let (kind, operands) = match &statement.kind {
            S::Import(_) => (LoweredOperationKind::Import, Vec::new()),
            S::Export(inner) => (
                LoweredOperationKind::Export,
                vec![self.statement(module, inner, Some(&id))],
            ),
            S::Function(declaration) => {
                closure_span = Some(declaration.name.span.merge(declaration.body.span));
                binding =
                    Some(self.binding(module, declaration.name, false, LoweredStorage::Module));
                let mut operands = declaration
                    .params
                    .iter()
                    .map(|parameter| self.parameter(module, parameter.name, parameter.span, &id))
                    .collect::<Vec<_>>();
                operands.extend(
                    declaration
                        .params
                        .iter()
                        .filter_map(|parameter| parameter.default.as_ref())
                        .map(|value| self.expression(module, value, Some(&id))),
                );
                operands.push(self.block(module, &declaration.body, Some(&id)));
                (LoweredOperationKind::Function, operands)
            }
            S::TypeAlias(_) => (LoweredOperationKind::TypeAlias, Vec::new()),
            S::Enum(_) => (LoweredOperationKind::Enum, Vec::new()),
            S::Record(declaration) => (
                LoweredOperationKind::Record,
                declaration
                    .fields
                    .iter()
                    .filter_map(|field| field.default.as_ref())
                    .map(|value| self.expression(module, value, Some(&id)))
                    .collect(),
            ),
            S::Newtype(_) => (LoweredOperationKind::Newtype, Vec::new()),
            S::Protocol(_) => (LoweredOperationKind::Protocol, Vec::new()),
            S::Impl(declaration) => (
                LoweredOperationKind::Implementation,
                declaration
                    .methods
                    .iter()
                    .map(|method| self.function(module, &method.decl, Some(&id)))
                    .collect(),
            ),
            S::Let {
                mutable,
                pattern,
                value,
                ..
            } => {
                let value = self.expression(module, value, Some(&id));
                let pattern_id = self.pattern(module, pattern, Some(&id), *mutable);
                (LoweredOperationKind::Let, vec![value, pattern_id])
            }
            S::Const { name, value, .. } => {
                binding = Some(self.binding(module, *name, false, LoweredStorage::Module));
                (
                    LoweredOperationKind::Constant,
                    vec![self.expression(module, value, Some(&id))],
                )
            }
            S::Assign { target, value, .. } => (
                LoweredOperationKind::Assignment,
                vec![
                    self.expression(module, target, Some(&id)),
                    self.expression(module, value, Some(&id)),
                ],
            ),
            S::Return(value) => {
                control = Some(LoweredControl {
                    kind: LoweredControlKind::Return,
                    target: None,
                    cleanup_ids: Vec::new(),
                });
                (
                    LoweredOperationKind::Return,
                    value
                        .as_ref()
                        .map(|value| vec![self.expression(module, value, Some(&id))])
                        .unwrap_or_default(),
                )
            }
            S::Defer(value) => {
                control = Some(LoweredControl {
                    kind: LoweredControlKind::Cleanup,
                    target: None,
                    cleanup_ids: vec![id.clone()],
                });
                (
                    LoweredOperationKind::Defer,
                    vec![self.expression(module, value, Some(&id))],
                )
            }
            S::Using { name, value, body } => {
                binding = Some(self.binding(module, *name, false, LoweredStorage::Local));
                (
                    LoweredOperationKind::Using,
                    vec![
                        self.expression(module, value, Some(&id)),
                        self.block(module, body, Some(&id)),
                    ],
                )
            }
            S::While { cond, body } => {
                control = Some(LoweredControl {
                    kind: LoweredControlKind::Loop,
                    target: Some(id.clone()),
                    cleanup_ids: Vec::new(),
                });
                (
                    LoweredOperationKind::While,
                    vec![
                        self.expression(module, cond, Some(&id)),
                        self.block(module, body, Some(&id)),
                    ],
                )
            }
            S::Break { value, .. } => {
                control = Some(LoweredControl {
                    kind: LoweredControlKind::Break,
                    target: None,
                    cleanup_ids: Vec::new(),
                });
                (
                    LoweredOperationKind::Break,
                    value
                        .as_ref()
                        .map(|value| vec![self.expression(module, value, Some(&id))])
                        .unwrap_or_default(),
                )
            }
            S::Continue { .. } => {
                control = Some(LoweredControl {
                    kind: LoweredControlKind::Continue,
                    target: None,
                    cleanup_ids: Vec::new(),
                });
                (LoweredOperationKind::Continue, Vec::new())
            }
            S::Expr(value) => (
                LoweredOperationKind::Expression(LoweredExpressionKind::Block),
                vec![self.expression(module, value, Some(&id))],
            ),
        };
        let operation_index = self.operations.len();
        self.push(
            &module.identity,
            LoweredOperation {
                id: id.clone(),
                module: module.identity.clone(),
                span: statement.span,
                parent: parent.map(str::to_string),
                role: if matches!(statement.kind, S::Defer(_)) {
                    LoweredRole::Cleanup
                } else {
                    LoweredRole::Statement
                },
                kind,
                operands,
                semantic_type: None,
                representation: None,
                binding,
                control,
                call: None,
                runtime_leaf: None,
            },
        );
        if let Some(span) = closure_span {
            self.closure_operations
                .insert((span.file.0, span.lo, span.hi), operation_index);
        }
        id
    }

    fn function(
        &mut self,
        module: &topaz_resolve::ResolvedModule,
        declaration: &ast::FunctionDecl,
        parent: Option<&str>,
    ) -> String {
        let span = declaration.name.span;
        let id = stable_id(&module.identity, LoweredRole::Declaration, "function", span);
        let closure_span = declaration.name.span.merge(declaration.body.span);
        let mut operands = declaration
            .params
            .iter()
            .map(|parameter| self.parameter(module, parameter.name, parameter.span, &id))
            .collect::<Vec<_>>();
        operands.extend(
            declaration
                .params
                .iter()
                .filter_map(|parameter| parameter.default.as_ref())
                .map(|value| self.expression(module, value, Some(&id))),
        );
        operands.push(self.block(module, &declaration.body, Some(&id)));
        let operation_index = self.operations.len();
        self.push(
            &module.identity,
            LoweredOperation {
                id: id.clone(),
                module: module.identity.clone(),
                span,
                parent: parent.map(str::to_string),
                role: LoweredRole::Declaration,
                kind: LoweredOperationKind::Function,
                operands,
                semantic_type: self.semantic(span, TypedNodeKind::Declaration),
                representation: None,
                binding: Some(self.binding(
                    module,
                    declaration.name,
                    false,
                    LoweredStorage::Module,
                )),
                control: None,
                call: None,
                runtime_leaf: None,
            },
        );
        self.closure_operations.insert(
            (closure_span.file.0, closure_span.lo, closure_span.hi),
            operation_index,
        );
        id
    }

    fn block(
        &mut self,
        module: &topaz_resolve::ResolvedModule,
        block: &ast::Block,
        parent: Option<&str>,
    ) -> String {
        let id = stable_id(
            &module.identity,
            LoweredRole::Expression,
            "block",
            block.span,
        );
        let mut operands = block
            .stmts
            .iter()
            .map(|statement| self.statement(module, statement, Some(&id)))
            .collect::<Vec<_>>();
        if let Some(tail) = &block.tail {
            operands.push(self.expression(module, tail, Some(&id)));
        }
        self.push(
            &module.identity,
            LoweredOperation {
                id: id.clone(),
                module: module.identity.clone(),
                span: block.span,
                parent: parent.map(str::to_string),
                role: LoweredRole::Expression,
                kind: LoweredOperationKind::Expression(LoweredExpressionKind::Block),
                operands,
                semantic_type: None,
                representation: None,
                binding: None,
                control: None,
                call: None,
                runtime_leaf: None,
            },
        );
        id
    }

    fn expression(
        &mut self,
        module: &topaz_resolve::ResolvedModule,
        expression: &ast::Expr,
        parent: Option<&str>,
    ) -> String {
        let tag = expression_tag(&expression.kind);
        let is_closure = matches!(&expression.kind, ast::ExprKind::Lambda { .. });
        let id = stable_id(
            &module.identity,
            LoweredRole::Expression,
            tag,
            expression.span,
        );
        let src = self.resolved.map.file(module.file).src();
        let mut control = None;
        let (kind, operands) = self.expression_parts(module, expression, &id, src, &mut control);
        let resolved_call = self.resolved_call(expression.span);
        let call = resolved_call.map(|fact| fact.plan.clone());
        let runtime_leaf = resolved_call
            .and_then(|fact| fact.target_identity.as_deref())
            .filter(|identity| {
                identity.starts_with("builtin::")
                    || identity.starts_with("std.")
                    || identity.starts_with("topaz.lispex-rule-handle/v1:")
            })
            .map(str::to_string)
            .or_else(|| runtime_leaf_for_expression(&kind).map(str::to_string));
        let semantic_type = self.semantic(expression.span, TypedNodeKind::Expression);
        let representation = semantic_type.as_ref().map(lowered_representation);
        let operation_index = self.operations.len();
        self.push(
            &module.identity,
            LoweredOperation {
                id: id.clone(),
                module: module.identity.clone(),
                span: expression.span,
                parent: parent.map(str::to_string),
                role: LoweredRole::Expression,
                kind: LoweredOperationKind::Expression(kind),
                operands,
                semantic_type,
                representation,
                binding: None,
                control,
                call,
                runtime_leaf,
            },
        );
        if is_closure {
            self.closure_operations.insert(
                (
                    expression.span.file.0,
                    expression.span.lo,
                    expression.span.hi,
                ),
                operation_index,
            );
        }
        id
    }

    fn expression_parts(
        &mut self,
        module: &topaz_resolve::ResolvedModule,
        expression: &ast::Expr,
        id: &str,
        src: &str,
        control: &mut Option<LoweredControl>,
    ) -> (LoweredExpressionKind, Vec<String>) {
        use ast::ExprKind as E;
        match &expression.kind {
            E::Int => (
                LoweredExpressionKind::Integer {
                    spelling: text(src, expression.span).to_string(),
                },
                Vec::new(),
            ),
            E::Float => (
                LoweredExpressionKind::Float {
                    spelling: text(src, expression.span).to_string(),
                },
                Vec::new(),
            ),
            E::Duration(_) => (
                LoweredExpressionKind::Duration {
                    spelling: text(src, expression.span).to_string(),
                },
                Vec::new(),
            ),
            E::Bool(value) => (LoweredExpressionKind::Boolean(*value), Vec::new()),
            E::Null => (LoweredExpressionKind::Null, Vec::new()),
            E::Unit => (LoweredExpressionKind::Unit, Vec::new()),
            E::String(literal) => {
                let mut operands = Vec::new();
                for part in &literal.parts {
                    match part {
                        ast::StringPart::Text(span) => {
                            let child = stable_id(
                                &module.identity,
                                LoweredRole::Expression,
                                "string-text",
                                *span,
                            );
                            self.push(
                                &module.identity,
                                LoweredOperation {
                                    id: child.clone(),
                                    module: module.identity.clone(),
                                    span: *span,
                                    parent: Some(id.to_string()),
                                    role: LoweredRole::Expression,
                                    kind: LoweredOperationKind::Expression(
                                        LoweredExpressionKind::StringText {
                                            text: text(src, *span).to_string(),
                                        },
                                    ),
                                    operands: Vec::new(),
                                    semantic_type: None,
                                    representation: None,
                                    binding: None,
                                    control: None,
                                    call: None,
                                    runtime_leaf: Some("string".to_string()),
                                },
                            );
                            operands.push(child);
                        }
                        ast::StringPart::Interpolation(value) => {
                            operands.push(self.expression(module, value, Some(id)));
                        }
                    }
                }
                (
                    LoweredExpressionKind::String {
                        tag: literal.tag.map(|span| text(src, span).to_string()),
                        multiline: literal.multiline,
                    },
                    operands,
                )
            }
            E::Ident => (
                LoweredExpressionKind::Identifier {
                    name: text(src, expression.span).to_string(),
                    target: self.reference(expression.span),
                },
                Vec::new(),
            ),
            E::Placeholder => (LoweredExpressionKind::Placeholder, Vec::new()),
            E::Paren(inner) => (
                LoweredExpressionKind::Parenthesized,
                vec![self.expression(module, inner, Some(id))],
            ),
            E::Block(block) => (
                LoweredExpressionKind::Block,
                vec![self.block(module, block, Some(id))],
            ),
            E::If {
                cond,
                then_block,
                else_branch,
            } => {
                *control = Some(LoweredControl {
                    kind: LoweredControlKind::Branch,
                    target: Some(id.to_string()),
                    cleanup_ids: Vec::new(),
                });
                let mut operands = vec![
                    self.expression(module, cond, Some(id)),
                    self.block(module, then_block, Some(id)),
                ];
                if let Some(branch) = else_branch {
                    operands.push(self.expression(module, branch, Some(id)));
                }
                (LoweredExpressionKind::If, operands)
            }
            E::Match { scrutinee, cases } => {
                *control = Some(LoweredControl {
                    kind: LoweredControlKind::Match,
                    target: Some(id.to_string()),
                    cleanup_ids: Vec::new(),
                });
                let mut operands = vec![self.expression(module, scrutinee, Some(id))];
                for case in cases {
                    operands.push(self.pattern(module, &case.pattern, Some(id), false));
                    if let Some(guard) = &case.guard {
                        operands.push(self.expression(module, guard, Some(id)));
                    }
                    match &case.body {
                        ast::CaseArmBody::Expr(value) => {
                            operands.push(self.expression(module, value, Some(id)));
                        }
                        ast::CaseArmBody::Return { value, .. } => {
                            if let Some(value) = value {
                                operands.push(self.expression(module, value, Some(id)));
                            }
                        }
                    }
                }
                (LoweredExpressionKind::Match, operands)
            }
            E::For {
                pattern,
                iter,
                body,
            } => {
                *control = Some(LoweredControl {
                    kind: LoweredControlKind::Loop,
                    target: Some(id.to_string()),
                    cleanup_ids: Vec::new(),
                });
                (
                    LoweredExpressionKind::For,
                    vec![
                        self.expression(module, iter, Some(id)),
                        self.pattern(module, pattern, Some(id), false),
                        self.block(module, body, Some(id)),
                    ],
                )
            }
            E::Loop { body, .. } => {
                *control = Some(LoweredControl {
                    kind: LoweredControlKind::Loop,
                    target: Some(id.to_string()),
                    cleanup_ids: Vec::new(),
                });
                (
                    LoweredExpressionKind::Loop,
                    vec![self.block(module, body, Some(id))],
                )
            }
            E::Concurrent {
                timeout,
                arms,
                else_block,
            } => {
                *control = Some(LoweredControl {
                    kind: LoweredControlKind::Concurrent,
                    target: Some(id.to_string()),
                    cleanup_ids: Vec::new(),
                });
                let mut operands = Vec::new();
                if let Some(timeout) = timeout {
                    operands.push(self.expression(module, timeout, Some(id)));
                }
                operands.extend(
                    arms.iter()
                        .map(|arm| self.expression(module, &arm.value, Some(id))),
                );
                if let Some(block) = else_block {
                    operands.push(self.block(module, block, Some(id)));
                }
                (LoweredExpressionKind::Concurrent, operands)
            }
            E::Call { callee, args, .. } => {
                let mut operands = vec![self.expression(module, callee, Some(id))];
                operands.extend(args.iter().map(|argument| match argument {
                    ast::CallArg::Positional(value)
                    | ast::CallArg::Spread(value)
                    | ast::CallArg::Named { value, .. } => self.expression(module, value, Some(id)),
                }));
                (LoweredExpressionKind::Call, operands)
            }
            E::Member { object, field } => (
                LoweredExpressionKind::Member {
                    name: text(src, field.span).to_string(),
                    target: self.reference(field.span),
                },
                vec![self.expression(module, object, Some(id))],
            ),
            E::Index { object, index } => (
                LoweredExpressionKind::Index,
                vec![
                    self.expression(module, object, Some(id)),
                    self.expression(module, index, Some(id)),
                ],
            ),
            E::OptionalAccess { object, field } => (
                LoweredExpressionKind::OptionalMember {
                    name: text(src, field.span).to_string(),
                    target: self.reference(field.span),
                },
                vec![self.expression(module, object, Some(id))],
            ),
            E::Try(inner) => {
                *control = Some(LoweredControl {
                    kind: LoweredControlKind::Propagate,
                    target: None,
                    cleanup_ids: Vec::new(),
                });
                (
                    LoweredExpressionKind::ResultPropagation,
                    vec![self.expression(module, inner, Some(id))],
                )
            }
            E::Unary { op, operand } => (
                LoweredExpressionKind::Unary {
                    operator: format!("{op:?}"),
                },
                vec![self.expression(module, operand, Some(id))],
            ),
            E::Binary { op, lhs, rhs } => (
                LoweredExpressionKind::Binary {
                    operator: format!("{op:?}"),
                },
                vec![
                    self.expression(module, lhs, Some(id)),
                    self.expression(module, rhs, Some(id)),
                ],
            ),
            E::Range {
                lo,
                hi,
                inclusive,
                step,
            } => {
                let mut operands = vec![
                    self.expression(module, lo, Some(id)),
                    self.expression(module, hi, Some(id)),
                ];
                if let Some(step) = step {
                    operands.push(self.expression(module, step, Some(id)));
                }
                (
                    LoweredExpressionKind::Range {
                        inclusive: *inclusive,
                    },
                    operands,
                )
            }
            E::Compose { lhs, rhs } => (
                LoweredExpressionKind::Compose,
                vec![
                    self.expression(module, lhs, Some(id)),
                    self.expression(module, rhs, Some(id)),
                ],
            ),
            E::Pipe { lhs, rhs } => {
                let mut operands = vec![self.expression(module, lhs, Some(id))];
                if let ast::PipeRhs::Expr(rhs) = rhs.as_ref() {
                    operands.push(self.expression(module, rhs, Some(id)));
                }
                (LoweredExpressionKind::Pipeline, operands)
            }
            E::Lambda { params, body } => {
                let mut operands = params
                    .iter()
                    .map(|parameter| self.parameter(module, parameter.name, parameter.span, id))
                    .collect::<Vec<_>>();
                operands.push(self.expression(module, body, Some(id)));
                (LoweredExpressionKind::Lambda, operands)
            }
            E::RecordLiteral { fields } => (
                LoweredExpressionKind::RecordLiteral,
                fields
                    .iter()
                    .map(|field| self.expression(module, &field.value, Some(id)))
                    .collect(),
            ),
            E::RecordUpdate {
                base,
                spread,
                fields,
            } => {
                let mut operands = vec![self.expression(module, base, Some(id))];
                if let Some(spread) = spread {
                    operands.push(self.expression(module, spread, Some(id)));
                }
                operands.extend(
                    fields
                        .iter()
                        .map(|field| self.expression(module, &field.value, Some(id))),
                );
                (LoweredExpressionKind::RecordUpdate, operands)
            }
            E::Array(elements) => (
                LoweredExpressionKind::Array,
                elements
                    .iter()
                    .map(|element| match element {
                        ast::ArrayElement::Expr(value) | ast::ArrayElement::Spread(value) => {
                            self.expression(module, value, Some(id))
                        }
                    })
                    .collect(),
            ),
            E::SetLiteral(values) => (
                LoweredExpressionKind::Set,
                values
                    .iter()
                    .map(|value| self.expression(module, value, Some(id)))
                    .collect(),
            ),
            E::MapLiteral(entries) => (
                LoweredExpressionKind::Map,
                entries
                    .iter()
                    .flat_map(|(key, value)| {
                        [
                            self.expression(module, key, Some(id)),
                            self.expression(module, value, Some(id)),
                        ]
                    })
                    .collect(),
            ),
            E::Comprehension {
                kind,
                clauses,
                body,
            } => {
                let mut operands = Vec::new();
                for clause in clauses {
                    match clause {
                        ast::CompClause::For { pattern, iter } => {
                            operands.push(self.expression(module, iter, Some(id)));
                            operands.push(self.pattern(module, pattern, Some(id), false));
                        }
                        ast::CompClause::If(value) => {
                            operands.push(self.expression(module, value, Some(id)));
                        }
                    }
                }
                match body.as_ref() {
                    ast::CompBody::Elem(value) => {
                        operands.push(self.expression(module, value, Some(id)));
                    }
                    ast::CompBody::Entry { key, value } => {
                        operands.push(self.expression(module, key, Some(id)));
                        operands.push(self.expression(module, value, Some(id)));
                    }
                }
                (
                    LoweredExpressionKind::Comprehension {
                        collection: format!("{kind:?}").to_lowercase(),
                    },
                    operands,
                )
            }
        }
    }

    fn pattern(
        &mut self,
        module: &topaz_resolve::ResolvedModule,
        pattern: &ast::Pattern,
        parent: Option<&str>,
        mutable: bool,
    ) -> String {
        let src = self.resolved.map.file(module.file).src();
        let contextual_constructor = matches!(&pattern.kind, ast::PatternKind::Binding(_))
            && self.semantic_types.contains_key(&(
                pattern.span.file.0,
                pattern.span.lo,
                pattern.span.hi,
                TypedNodeKind::Pattern,
            ))
            && !self.semantic_types.contains_key(&(
                pattern.span.file.0,
                pattern.span.lo,
                pattern.span.hi,
                TypedNodeKind::Binding,
            ));
        let tag = if contextual_constructor {
            "constructor"
        } else {
            pattern_tag(&pattern.kind)
        };
        let id = stable_id(&module.identity, LoweredRole::Pattern, tag, pattern.span);
        let (kind, operands, binding) = match &pattern.kind {
            ast::PatternKind::Or(values) => (
                LoweredPatternKind::Alternatives,
                values
                    .iter()
                    .map(|value| self.pattern(module, value, Some(&id), false))
                    .collect(),
                None,
            ),
            ast::PatternKind::Wildcard => (LoweredPatternKind::Wildcard, Vec::new(), None),
            ast::PatternKind::Literal(value) => (
                LoweredPatternKind::Literal,
                vec![self.expression(module, value, Some(&id))],
                None,
            ),
            ast::PatternKind::Range { lo, hi, inclusive } => (
                LoweredPatternKind::Range {
                    inclusive: *inclusive,
                },
                vec![
                    self.expression(module, lo, Some(&id)),
                    self.expression(module, hi, Some(&id)),
                ],
                None,
            ),
            ast::PatternKind::Binding(name) if contextual_constructor => (
                LoweredPatternKind::Constructor {
                    name: text(src, name.span).to_string(),
                },
                Vec::new(),
                None,
            ),
            ast::PatternKind::Binding(name) => (
                LoweredPatternKind::Binding {
                    name: text(src, name.span).to_string(),
                },
                Vec::new(),
                Some(self.binding(module, *name, mutable, LoweredStorage::Local)),
            ),
            ast::PatternKind::Typed { name, .. } => (
                LoweredPatternKind::TypedBinding {
                    name: text(src, name.span).to_string(),
                },
                Vec::new(),
                Some(self.binding(module, *name, mutable, LoweredStorage::Local)),
            ),
            ast::PatternKind::Constructor { name, args } => (
                LoweredPatternKind::Constructor {
                    name: text(src, name.span).to_string(),
                },
                args.iter()
                    .map(|value| self.pattern(module, value, Some(&id), false))
                    .collect(),
                None,
            ),
            ast::PatternKind::List(values) => (
                LoweredPatternKind::List,
                values
                    .iter()
                    .filter_map(|value| match value {
                        ast::ListPatternElem::Pattern(value) => {
                            Some(self.pattern(module, value, Some(&id), false))
                        }
                        ast::ListPatternElem::Rest(Some(value)) => {
                            Some(self.pattern(module, value, Some(&id), false))
                        }
                        ast::ListPatternElem::Rest(None) => None,
                    })
                    .collect(),
                None,
            ),
            ast::PatternKind::Record(fields) => (
                LoweredPatternKind::Record,
                fields
                    .iter()
                    .filter_map(|field| field.pattern.as_ref())
                    .map(|value| self.pattern(module, value, Some(&id), false))
                    .collect(),
                None,
            ),
            ast::PatternKind::NominalRecord { name, fields } => (
                LoweredPatternKind::NominalRecord {
                    name: text(src, name.span).to_string(),
                },
                fields
                    .iter()
                    .filter_map(|field| field.pattern.as_ref())
                    .map(|value| self.pattern(module, value, Some(&id), false))
                    .collect(),
                None,
            ),
        };
        let semantic_type = self.semantic(pattern.span, TypedNodeKind::Pattern);
        let representation = self
            .representation(pattern.span)
            .or_else(|| semantic_type.as_ref().map(lowered_representation));
        self.push(
            &module.identity,
            LoweredOperation {
                id: id.clone(),
                module: module.identity.clone(),
                span: pattern.span,
                parent: parent.map(str::to_string),
                role: LoweredRole::Pattern,
                kind: LoweredOperationKind::Pattern(kind),
                operands,
                semantic_type,
                representation,
                binding,
                control: None,
                call: None,
                runtime_leaf: Some("pattern".to_string()),
            },
        );
        id
    }

    fn binding(
        &self,
        module: &topaz_resolve::ResolvedModule,
        name: ast::Ident,
        mutable: bool,
        storage: LoweredStorage,
    ) -> LoweredBinding {
        let src = self.resolved.map.file(module.file).src();
        LoweredBinding {
            name: text(src, name.span).to_string(),
            mutable,
            storage,
            declaration_identity: Some(self.reference(name.span).unwrap_or_else(|| {
                format!(
                    "source:{}:{}:{}",
                    name.span.file.0, name.span.lo, name.span.hi
                )
            })),
        }
    }

    fn parameter(
        &mut self,
        module: &topaz_resolve::ResolvedModule,
        name: ast::Ident,
        span: Span,
        parent: &str,
    ) -> String {
        let id = stable_id(&module.identity, LoweredRole::Binding, "parameter", span);
        self.push(
            &module.identity,
            LoweredOperation {
                id: id.clone(),
                module: module.identity.clone(),
                span,
                parent: Some(parent.to_string()),
                role: LoweredRole::Binding,
                kind: LoweredOperationKind::Pattern(LoweredPatternKind::Binding {
                    name: self
                        .binding(module, name, false, LoweredStorage::Parameter)
                        .name,
                }),
                operands: Vec::new(),
                semantic_type: self.semantic(span, TypedNodeKind::Pattern),
                representation: self.representation(span),
                binding: Some(self.binding(module, name, false, LoweredStorage::Parameter)),
                control: None,
                call: None,
                runtime_leaf: None,
            },
        );
        id
    }

    fn lower_capture(&mut self, capture: &topaz_hir::TypedCapture) -> Result<(), LowerError> {
        let resolved_parent = self
            .closure_operations
            .get(&(
                capture.closure_span.file.0,
                capture.closure_span.lo,
                capture.closure_span.hi,
            ))
            .and_then(|index| self.operations.get(*index))
            .map(|operation| operation.id.clone());
        let parent = resolved_parent.ok_or_else(|| LowerError::UnknownCaptureParent {
            module: capture.module.clone(),
            name: capture.name.clone(),
            lo: capture.closure_span.lo,
            hi: capture.closure_span.hi,
        })?;
        let id = format!(
            "{}:parent:{parent}",
            stable_id(
                &capture.module,
                LoweredRole::Binding,
                "capture",
                capture.reference_span,
            )
        );
        self.push(
            &capture.module,
            LoweredOperation {
                id,
                module: capture.module.clone(),
                span: capture.reference_span,
                parent: Some(parent),
                role: LoweredRole::Binding,
                kind: LoweredOperationKind::Pattern(LoweredPatternKind::Binding {
                    name: capture.name.clone(),
                }),
                operands: Vec::new(),
                semantic_type: Some(capture.ty.clone()),
                representation: self.representation(capture.declaration_span),
                binding: Some(LoweredBinding {
                    name: capture.name.clone(),
                    mutable: false,
                    storage: LoweredStorage::Captured,
                    declaration_identity: Some(format!(
                        "source:{}:{}:{}",
                        capture.declaration_span.file.0,
                        capture.declaration_span.lo,
                        capture.declaration_span.hi
                    )),
                }),
                control: None,
                call: None,
                runtime_leaf: None,
            },
        );
        Ok(())
    }

    fn semantic(&self, span: Span, kind: TypedNodeKind) -> Option<SemanticType> {
        self.semantic_types
            .get(&(span.file.0, span.lo, span.hi, kind))
            .map(|ty| (*ty).clone())
    }

    fn representation(&self, span: Span) -> Option<topaz_hir::MonoTy> {
        self.representations
            .get(&(span.file.0, span.lo, span.hi))
            .copied()
    }

    fn reference(&self, span: Span) -> Option<String> {
        self.references
            .get(&(span.file.0, span.lo, span.hi))
            .copied()
            .map(reference_identity)
    }

    fn resolved_call(&self, span: Span) -> Option<&topaz_hir::TypedCall> {
        self.calls.get(&(span.file.0, span.lo, span.hi)).copied()
    }

    fn push(&mut self, module: &str, operation: LoweredOperation) {
        self.module_operations
            .entry(module.to_string())
            .or_default()
            .push(operation.id.clone());
        self.operations.push(operation);
    }
}

fn resolved_reference_index(
    resolved: &ResolveOutput,
) -> BTreeMap<(u32, u32, u32), &ResolvedReferenceFact> {
    let mut index = BTreeMap::new();
    for reference in &resolved.name_facts.references {
        index
            .entry((reference.file.0, reference.span.lo, reference.span.hi))
            .or_insert(reference);
    }
    index
}

fn enriched_calls(
    resolved: &ResolveOutput,
    calls: &[topaz_hir::TypedCall],
) -> Vec<topaz_hir::TypedCall> {
    let mut references_by_file: BTreeMap<u32, Vec<&ResolvedReferenceFact>> = BTreeMap::new();
    for reference in &resolved.name_facts.references {
        references_by_file
            .entry(reference.file.0)
            .or_default()
            .push(reference);
    }
    for references in references_by_file.values_mut() {
        references.sort_by_key(|reference| (reference.span.lo, reference.span.hi));
    }
    let mut calls = calls.to_vec();
    for call in &mut calls {
        let target = references_by_file
            .get(&call.callee_span.file.0)
            .and_then(|references| {
                let start =
                    references.partition_point(|reference| reference.span.lo < call.callee_span.lo);
                let end = references
                    .partition_point(|reference| reference.span.lo <= call.callee_span.hi);
                let mut targets = references[start..end].iter().filter(|reference| {
                    reference.span.hi <= call.callee_span.hi
                        && call
                            .plan
                            .admits_callee_reference(&reference.name, reference.span)
                });
                let first_target = targets.next().copied();
                first_target.filter(|_| targets.next().is_none())
            });
        // The checker may already have replaced an ordinary resolver identity
        // with a capability-scoped call identity.  In particular, generated
        // `std.lispex.rules` factories carry the exact locked rule name in the
        // target identity.  Resolution can enrich an unlabelled call, but it
        // must never erase a stronger checked fact.
        if call.target_identity.is_none() {
            call.target_identity = target.map(reference_identity);
        }
    }
    calls
}

struct ResolutionCaptureTarget {
    declaration_scope: u32,
    declaration_span: Span,
    name: String,
    ty: SemanticType,
    ambient: bool,
}

fn resolution_capture_target(
    declarations: &BTreeMap<(u32, u32, u32), &topaz_resolve::ResolvedDeclarationFact>,
    namespace_imports: &BTreeMap<(u32, &str), &topaz_resolve::ResolvedDeclarationFact>,
    declared_nodes: &BTreeMap<(u32, u32, u32), &topaz_hir::TypedNode>,
    expression_nodes: &BTreeMap<(u32, u32, u32), &topaz_hir::TypedNode>,
    reference: &ResolvedReferenceFact,
) -> Option<ResolutionCaptureTarget> {
    if reference.namespace != topaz_resolve::ResolvedNamespace::Value {
        return None;
    }
    if reference.role == topaz_resolve::ResolvedReferenceRole::NamespaceMember {
        let alias = reference.name.split_once('.')?.0;
        let declaration = namespace_imports.get(&(reference.file.0, alias)).copied()?;
        return Some(ResolutionCaptureTarget {
            declaration_scope: declaration.scope_ordinal,
            declaration_span: declaration.span,
            name: declaration.name.clone(),
            ty: SemanticType::Unknown,
            ambient: true,
        });
    }

    let (target_file, target_span) = (reference.target_file?, reference.target_span?);
    if target_file != reference.file {
        return None;
    }
    let declaration = declarations.get(&(target_file.0, target_span.lo, target_span.hi))?;
    let node = declared_nodes
        .get(&(target_span.file.0, target_span.lo, target_span.hi))
        .copied()
        // Selected imports have resolver declarations but no local checker
        // declaration node. Their resolved use still carries the checked type.
        .or_else(|| {
            expression_nodes
                .get(&(reference.span.file.0, reference.span.lo, reference.span.hi))
                .copied()
        })?;
    Some(ResolutionCaptureTarget {
        declaration_scope: declaration.scope_ordinal,
        declaration_span: target_span,
        name: reference.name.clone(),
        ty: node.ty.clone(),
        ambient: node.ty.has_hole(),
    })
}

/// Derive the complete runtime capture chain from resolved lexical ownership.
/// Typed observations and lowering call this same authority, so every
/// intervening function or lambda receives the value it must pass inward.
pub fn derive_resolution_captures(
    resolved: &ResolveOutput,
    typed: &topaz_hir::TypedUnit,
) -> Vec<topaz_hir::TypedCapture> {
    let scopes = resolved
        .name_facts
        .scopes
        .iter()
        .map(|scope| ((scope.file.0, scope.ordinal), scope))
        .collect::<BTreeMap<_, _>>();
    let mut declarations = BTreeMap::new();
    let mut namespace_imports = BTreeMap::new();
    for declaration in &resolved.name_facts.declarations {
        declarations.insert(
            (declaration.file.0, declaration.span.lo, declaration.span.hi),
            declaration,
        );
        if declaration.namespace == topaz_resolve::ResolvedNamespace::Module
            && declaration.kind == topaz_resolve::ResolvedDeclarationKind::NamespaceImport
        {
            namespace_imports
                .entry((declaration.file.0, declaration.name.as_str()))
                .or_insert(declaration);
        }
    }
    let module_identities = resolved
        .modules
        .iter()
        .map(|module| (module.file.0, module.identity.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut declared_nodes = BTreeMap::new();
    let mut expression_nodes = BTreeMap::new();
    for node in &typed.nodes {
        let key = (node.span.file.0, node.span.lo, node.span.hi);
        match node.kind {
            topaz_hir::TypedNodeKind::Binding | topaz_hir::TypedNodeKind::Declaration => {
                declared_nodes.entry(key).or_insert(node);
            }
            topaz_hir::TypedNodeKind::Expression => {
                expression_nodes.entry(key).or_insert(node);
            }
            topaz_hir::TypedNodeKind::Pattern | topaz_hir::TypedNodeKind::Type => {}
        }
    }
    let mut captures = Vec::new();
    let mut scope_ancestors = BTreeMap::<(u32, u32), BTreeSet<u32>>::new();

    for reference in &resolved.name_facts.references {
        let Some(target) = resolution_capture_target(
            &declarations,
            &namespace_imports,
            &declared_nodes,
            &expression_nodes,
            reference,
        ) else {
            continue;
        };
        let module = module_identities
            .get(&reference.file.0)
            .copied()
            .unwrap_or("<unknown>");
        let declaration_ancestors = scope_ancestors
            .entry((reference.file.0, target.declaration_scope))
            .or_insert_with(|| {
                let mut ancestors = BTreeSet::new();
                let mut current = Some(target.declaration_scope);
                while let Some(ordinal) = current {
                    ancestors.insert(ordinal);
                    current = scopes
                        .get(&(reference.file.0, ordinal))
                        .and_then(|scope| scope.parent_ordinal);
                }
                ancestors
            });
        let mut current = Some(reference.scope_ordinal);
        while let Some(ordinal) = current {
            let Some(scope) = scopes.get(&(reference.file.0, ordinal)) else {
                break;
            };
            if matches!(
                scope.kind,
                topaz_resolve::ResolvedScopeKind::Function
                    | topaz_resolve::ResolvedScopeKind::Lambda
            ) && !declaration_ancestors.contains(&scope.ordinal)
            {
                // Thread a free reference through every intervening closure.
                // The checked backend consumes this explicit capture chain and
                // never has to rediscover lexical ownership from syntax.
                captures.push(topaz_hir::TypedCapture {
                    module: module.to_string(),
                    closure_span: scope.owner,
                    reference_span: reference.span,
                    declaration_span: target.declaration_span,
                    name: target.name.clone(),
                    ty: target.ty.clone(),
                    ambient: target.ambient,
                });
            }
            current = scope.parent_ordinal;
        }
    }
    captures.sort_by(|left, right| {
        left.module
            .cmp(&right.module)
            .then_with(|| left.closure_span.file.0.cmp(&right.closure_span.file.0))
            .then_with(|| left.closure_span.lo.cmp(&right.closure_span.lo))
            .then_with(|| left.closure_span.hi.cmp(&right.closure_span.hi))
            .then_with(|| left.reference_span.lo.cmp(&right.reference_span.lo))
            .then_with(|| left.reference_span.hi.cmp(&right.reference_span.hi))
            .then_with(|| left.declaration_span.lo.cmp(&right.declaration_span.lo))
            .then_with(|| left.declaration_span.hi.cmp(&right.declaration_span.hi))
            .then_with(|| left.name.cmp(&right.name))
    });
    captures.dedup_by(|left, right| {
        left.module == right.module
            && left.closure_span == right.closure_span
            && left.reference_span == right.reference_span
            && left.declaration_span == right.declaration_span
            && left.name == right.name
    });
    captures
}

fn sort_operations(resolved: &ResolveOutput, operations: &mut [LoweredOperation]) {
    let module_ordinals = resolved
        .modules
        .iter()
        .enumerate()
        .map(|(ordinal, module)| (module.identity.as_str(), ordinal))
        .collect::<BTreeMap<_, _>>();
    operations.sort_by(|left, right| {
        let ordinal = |identity: &str| module_ordinals.get(identity).copied().unwrap_or(usize::MAX);
        (
            ordinal(&left.module),
            left.span.file.0,
            left.span.lo,
            left.span.hi,
            &left.id,
        )
            .cmp(&(
                ordinal(&right.module),
                right.span.file.0,
                right.span.lo,
                right.span.hi,
                &right.id,
            ))
    });
}

fn stable_id(module: &str, role: LoweredRole, tag: &str, span: Span) -> String {
    format!(
        "op:{module}:{}:{}:{}:{role:?}:{tag}",
        span.file.0, span.lo, span.hi
    )
}

fn text(source: &str, span: Span) -> &str {
    &source[span.lo as usize..span.hi as usize]
}

fn reference_identity(reference: &ResolvedReferenceFact) -> String {
    if let (Some(module), Some(name)) = (&reference.target_module, &reference.target_name) {
        format!("{module}::{name}")
    } else if let (Some(file), Some(span)) = (reference.target_file, reference.target_span) {
        format!("source:{}:{}:{}", file.0, span.lo, span.hi)
    } else {
        format!("builtin::{}", reference.name)
    }
}

fn expression_tag(kind: &ast::ExprKind) -> &'static str {
    use ast::ExprKind as E;
    match kind {
        E::Int => "integer",
        E::Float => "float",
        E::Duration(_) => "duration",
        E::Bool(_) => "boolean",
        E::Null => "null",
        E::Unit => "unit",
        E::String(_) => "string",
        E::Ident => "identifier",
        E::Placeholder => "placeholder",
        E::Paren(_) => "parenthesized",
        E::Block(_) => "block-expression",
        E::If { .. } => "if",
        E::Match { .. } => "match",
        E::For { .. } => "for",
        E::Loop { .. } => "loop",
        E::Concurrent { .. } => "concurrent",
        E::Call { .. } => "call",
        E::Member { .. } => "member",
        E::Index { .. } => "index",
        E::OptionalAccess { .. } => "optional-member",
        E::Try(_) => "result-propagation",
        E::Unary { .. } => "unary",
        E::Binary { .. } => "binary",
        E::Range { .. } => "range",
        E::Compose { .. } => "compose",
        E::Pipe { .. } => "pipeline",
        E::Lambda { .. } => "lambda",
        E::RecordLiteral { .. } => "record-literal",
        E::RecordUpdate { .. } => "record-update",
        E::Array(_) => "array",
        E::SetLiteral(_) => "set",
        E::MapLiteral(_) => "map",
        E::Comprehension { .. } => "comprehension",
    }
}

fn runtime_leaf_for_expression(kind: &LoweredExpressionKind) -> Option<&'static str> {
    use LoweredExpressionKind as E;
    match kind {
        E::String { tag: Some(_), .. } => Some("template"),
        E::String { .. } | E::StringText { .. } => Some("string"),
        E::If | E::Match | E::For | E::Loop | E::Block | E::Parenthesized => None,
        E::Concurrent => Some("concurrent"),
        E::Call => Some("call"),
        E::Member { .. } | E::OptionalMember { .. } => Some("member"),
        E::Index => Some("collection"),
        E::ResultPropagation => Some("result"),
        E::Unary { .. } => Some("unary"),
        E::Binary { .. } => Some("binary"),
        E::Range { .. } => Some("range"),
        E::Compose | E::Pipeline | E::Lambda => Some("callable"),
        E::RecordLiteral | E::RecordUpdate => Some("record"),
        E::Array => Some("array"),
        E::Set => Some("set"),
        E::Map => Some("map"),
        E::Comprehension { collection } => match collection.as_str() {
            "array" => Some("array"),
            "set" => Some("set"),
            "map" => Some("map"),
            _ => Some("collection"),
        },
        E::Integer { .. }
        | E::Float { .. }
        | E::Duration { .. }
        | E::Boolean(_)
        | E::Null
        | E::Unit
        | E::Identifier { .. }
        | E::Placeholder => None,
    }
}

fn lowered_representation(ty: &SemanticType) -> topaz_hir::MonoTy {
    match ty {
        SemanticType::Primitive(topaz_hir::SemanticPrimitive::Int)
        | SemanticType::Literal(topaz_hir::SemanticLiteral::Int(_)) => topaz_hir::MonoTy::I64,
        SemanticType::Primitive(topaz_hir::SemanticPrimitive::Float)
        | SemanticType::Literal(topaz_hir::SemanticLiteral::Float(_)) => topaz_hir::MonoTy::F64,
        SemanticType::Primitive(topaz_hir::SemanticPrimitive::Bool)
        | SemanticType::Literal(topaz_hir::SemanticLiteral::Bool(_)) => topaz_hir::MonoTy::Bool,
        SemanticType::Primitive(topaz_hir::SemanticPrimitive::Unit) => topaz_hir::MonoTy::Unit,
        SemanticType::Bytes => topaz_hir::MonoTy::BytesHandle,
        SemanticType::ByteBuffer => topaz_hir::MonoTy::ByteBufferHandle,
        _ => topaz_hir::MonoTy::Boxed,
    }
}

fn pattern_tag(kind: &ast::PatternKind) -> &'static str {
    match kind {
        ast::PatternKind::Or(_) => "alternatives",
        ast::PatternKind::Wildcard => "wildcard",
        ast::PatternKind::Literal(_) => "literal",
        ast::PatternKind::Range { .. } => "range",
        ast::PatternKind::Binding(_) => "binding",
        ast::PatternKind::Typed { .. } => "typed-binding",
        ast::PatternKind::Constructor { .. } => "constructor",
        ast::PatternKind::List(_) => "list",
        ast::PatternKind::Record(_) => "record",
        ast::PatternKind::NominalRecord { .. } => "nominal-record",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use topaz_resolve::{InMemoryProvider, resolve_with_version};
    use topaz_syntax::LangVersion;

    #[test]
    fn checked_lowering_records_parameters_captures_calls_and_exact_runtime_inputs() {
        let mut provider = InMemoryProvider::new();
        provider.add_file(
            "main.tpz",
            "function apply(value: int, f: (int) -> int) -> int { f(value) }\n\
             let offset = 2\n\
             let add = (value: int) => value + offset\n\
             print(\"{apply(40, add)}\")\n",
        );
        let resolved = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
        assert!(
            resolved.diagnostics.is_empty(),
            "{:?}",
            resolved.diagnostics
        );
        let modules = resolved
            .modules
            .iter()
            .map(|module| topaz_check::UnitModule {
                identity: module.identity.clone(),
                is_entry: module.is_entry,
                is_extern: module.is_extern,
                is_generated_std: module.is_generated_std,
                extern_replay_error: module.extern_replay_error.clone(),
                src: resolved.map.file(module.file).src(),
                program: &module.program,
            })
            .collect::<Vec<_>>();
        let checked = topaz_check::check_unit_typed_with_version(&modules, LangVersion::CURRENT);
        assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
        let lowered = lower_checked(&resolved, &checked).expect("lowered");

        assert!(lowered.operations.iter().any(|operation| {
            operation
                .binding
                .as_ref()
                .is_some_and(|binding| binding.storage == LoweredStorage::Parameter)
        }));
        assert!(lowered.operations.iter().any(|operation| {
            operation
                .binding
                .as_ref()
                .is_some_and(|binding| binding.storage == LoweredStorage::Captured)
        }));
        assert!(lowered.operations.iter().any(|operation| {
            operation.call.is_some()
                && matches!(
                    operation.kind,
                    LoweredOperationKind::Expression(LoweredExpressionKind::Call)
                )
        }));
        assert!(
            lowered
                .runtime
                .leaves
                .iter()
                .any(|leaf| { leaf.identity == "builtin::print" || leaf.identity == "call" })
        );
        assert_eq!(lowered.runtime.templates.len(), 2);
        assert!(
            lowered
                .runtime
                .templates
                .iter()
                .all(|template| template.sha256.starts_with("sha256:"))
        );
        assert!(lowered.modules.iter().all(|module| !module.text.is_empty()));
    }

    #[test]
    fn capture_derivation_threads_each_runtime_closure_once() {
        let mut provider = InMemoryProvider::new();
        provider.add_file(
            "main.tpz",
            "record Marker { value: int }\n\
             let global = 1\n\
             function identity<T>(value: T) -> T { value }\n\
             let callableAlias = identity\n\
             function make(seed: int) {\n\
               let localAlias = callableAlias\n\
               () => {\n\
                 let middle = seed\n\
                 () => localAlias<int>(global + middle)\n\
               }\n\
             }\n\
             let typedOnly = () => {\n\
               let value: Marker = Marker { value: 1 }\n\
               value.value\n\
             }\n",
        );
        let resolved = resolve_with_version(&provider, "main.tpz", None, LangVersion::CURRENT);
        assert!(
            resolved.diagnostics.is_empty(),
            "{:?}",
            resolved.diagnostics
        );
        let modules = resolved
            .modules
            .iter()
            .map(|module| topaz_check::UnitModule {
                identity: module.identity.clone(),
                is_entry: module.is_entry,
                is_extern: module.is_extern,
                is_generated_std: module.is_generated_std,
                extern_replay_error: module.extern_replay_error.clone(),
                src: resolved.map.file(module.file).src(),
                program: &module.program,
            })
            .collect::<Vec<_>>();
        let checked = topaz_check::check_unit_typed_with_version(&modules, LangVersion::CURRENT);
        assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
        let typed = checked.typed_hir.as_ref().expect("typed HIR");
        let captures = derive_resolution_captures(&resolved, typed);
        let mut counts = BTreeMap::<String, usize>::new();
        for capture in &captures {
            *counts.entry(capture.name.clone()).or_default() += 1;
        }
        assert_eq!(
            counts,
            BTreeMap::from([
                ("callableAlias".to_string(), 1),
                ("global".to_string(), 3),
                ("localAlias".to_string(), 2),
                ("middle".to_string(), 1),
                ("seed".to_string(), 1),
            ])
        );
        assert!(captures.iter().all(|capture| capture.name != "Marker"));

        let lowered = lower_checked(&resolved, &checked).expect("lowered");
        assert_eq!(lowered.captures, captures);
    }
}
