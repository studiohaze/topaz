use std::rc::Rc;

use topaz_diag::Span;
use topaz_hir::emission as e;
use topaz_syntax::ast as a;

pub(crate) fn program(
    identity: &str,
    program: &a::Program,
    source: &str,
) -> (e::Program, e::LoweredText) {
    let mut text = e::LoweredText::with_identity(identity);
    let program = program_node(program, source, &mut text);
    (program, text)
}

fn atom(text: &mut e::LoweredText, source: &str, span: Span) {
    text.insert(
        span,
        source
            .get(span.lo as usize..span.hi as usize)
            .unwrap_or_default(),
    );
}

fn ident(value: a::Ident, source: &str, text: &mut e::LoweredText) -> e::Ident {
    atom(text, source, value.span);
    e::Ident { span: value.span }
}

fn program_node(value: &a::Program, source: &str, text: &mut e::LoweredText) -> e::Program {
    atom(text, source, value.span);
    e::Program {
        items: value
            .items
            .iter()
            .map(|value| stmt(value, source, text))
            .collect(),
        span: value.span,
    }
}

fn stmt(value: &a::Stmt, source: &str, text: &mut e::LoweredText) -> e::Stmt {
    atom(text, source, value.span);
    e::Stmt {
        kind: match &value.kind {
            a::StmtKind::Import(value) => e::StmtKind::Import(import_item(value, source, text)),
            a::StmtKind::Export(value) => e::StmtKind::Export(Rc::new(stmt(value, source, text))),
            a::StmtKind::Function(value) => e::StmtKind::Function(function(value, source, text)),
            a::StmtKind::TypeAlias(value) => {
                e::StmtKind::TypeAlias(type_alias(value, source, text))
            }
            a::StmtKind::Enum(value) => e::StmtKind::Enum(enum_decl(value, source, text)),
            a::StmtKind::Record(value) => e::StmtKind::Record(record_decl(value, source, text)),
            a::StmtKind::Newtype(value) => e::StmtKind::Newtype(newtype_decl(value, source, text)),
            a::StmtKind::Impl(value) => e::StmtKind::Impl(impl_decl(value, source, text)),
            a::StmtKind::Protocol(value) => {
                e::StmtKind::Protocol(protocol_decl(value, source, text))
            }
            a::StmtKind::Let {
                mutable,
                pattern: value,
                ty,
                value: initializer,
            } => e::StmtKind::Let {
                mutable: *mutable,
                pattern: pattern(value, source, text),
                ty: ty.as_ref().map(|value| ty_node(value, source, text)),
                value: expr(initializer, source, text),
            },
            a::StmtKind::Const { name, ty, value } => e::StmtKind::Const {
                name: ident(*name, source, text),
                ty: ty.as_ref().map(|value| ty_node(value, source, text)),
                value: expr(value, source, text),
            },
            a::StmtKind::Assign { target, op, value } => e::StmtKind::Assign {
                target: expr(target, source, text),
                op: assign_op(*op),
                value: expr(value, source, text),
            },
            a::StmtKind::Return(value) => {
                e::StmtKind::Return(value.as_ref().map(|value| expr(value, source, text)))
            }
            a::StmtKind::Defer(value) => e::StmtKind::Defer(expr(value, source, text)),
            a::StmtKind::Using { name, value, body } => e::StmtKind::Using {
                name: ident(*name, source, text),
                value: expr(value, source, text),
                body: Rc::new(block(body, source, text)),
            },
            a::StmtKind::While { cond, body } => e::StmtKind::While {
                cond: expr(cond, source, text),
                body: Rc::new(block(body, source, text)),
            },
            a::StmtKind::Break { label, value } => e::StmtKind::Break {
                label: label.map(|value| ident(value, source, text)),
                value: value.as_ref().map(|value| expr(value, source, text)),
            },
            a::StmtKind::Continue { label } => e::StmtKind::Continue {
                label: label.map(|value| ident(value, source, text)),
            },
            a::StmtKind::Expr(value) => e::StmtKind::Expr(expr(value, source, text)),
        },
        span: value.span,
    }
}

fn function(value: &a::FunctionDecl, source: &str, text: &mut e::LoweredText) -> e::FunctionDecl {
    text.record_closure_body(value.name.span.merge(value.body.span), value.body.span);
    e::FunctionDecl {
        name: ident(value.name, source, text),
        type_params: value
            .type_params
            .iter()
            .map(|value| ident(*value, source, text))
            .collect(),
        type_param_bounds: value
            .type_param_bounds
            .iter()
            .map(|values| {
                values
                    .iter()
                    .map(|value| ident(*value, source, text))
                    .collect()
            })
            .collect(),
        params: value
            .params
            .iter()
            .map(|value| param(value, source, text))
            .collect(),
        return_type: value
            .return_type
            .as_ref()
            .map(|value| ty_node(value, source, text)),
        body: Rc::new(block(&value.body, source, text)),
    }
}

fn param(value: &a::Param, source: &str, text: &mut e::LoweredText) -> e::Param {
    atom(text, source, value.span);
    e::Param {
        name: ident(value.name, source, text),
        ty: ty_node(&value.ty, source, text),
        default: value
            .default
            .as_ref()
            .map(|value| expr(value, source, text)),
        variadic: value.variadic,
        span: value.span,
    }
}

fn type_alias(value: &a::TypeAlias, source: &str, text: &mut e::LoweredText) -> e::TypeAlias {
    e::TypeAlias {
        name: ident(value.name, source, text),
        type_params: value
            .type_params
            .iter()
            .map(|value| ident(*value, source, text))
            .collect(),
        ty: ty_node(&value.ty, source, text),
    }
}

fn enum_decl(value: &a::EnumDecl, source: &str, text: &mut e::LoweredText) -> e::EnumDecl {
    e::EnumDecl {
        name: ident(value.name, source, text),
        type_params: value
            .type_params
            .iter()
            .map(|value| ident(*value, source, text))
            .collect(),
        variants: value
            .variants
            .iter()
            .map(|value| {
                atom(text, source, value.span);
                e::EnumVariant {
                    name: ident(value.name, source, text),
                    payload: value.payload.as_ref().map(|types| {
                        types
                            .iter()
                            .map(|value| ty_node(value, source, text))
                            .collect()
                    }),
                    span: value.span,
                }
            })
            .collect(),
        derives: value
            .derives
            .iter()
            .map(|value| ident(*value, source, text))
            .collect(),
    }
}

fn record_decl(value: &a::RecordDecl, source: &str, text: &mut e::LoweredText) -> e::RecordDecl {
    e::RecordDecl {
        name: ident(value.name, source, text),
        type_params: value
            .type_params
            .iter()
            .map(|value| ident(*value, source, text))
            .collect(),
        fields: value
            .fields
            .iter()
            .map(|value| {
                atom(text, source, value.span);
                e::RecordFieldDecl {
                    name: ident(value.name, source, text),
                    ty: ty_node(&value.ty, source, text),
                    default: value
                        .default
                        .as_ref()
                        .map(|value| expr(value, source, text)),
                    span: value.span,
                }
            })
            .collect(),
        derives: value
            .derives
            .iter()
            .map(|value| ident(*value, source, text))
            .collect(),
    }
}

fn newtype_decl(value: &a::NewtypeDecl, source: &str, text: &mut e::LoweredText) -> e::NewtypeDecl {
    e::NewtypeDecl {
        name: ident(value.name, source, text),
        type_params: value
            .type_params
            .iter()
            .map(|value| ident(*value, source, text))
            .collect(),
        base: ty_node(&value.base, source, text),
    }
}

fn impl_decl(value: &a::ImplDecl, source: &str, text: &mut e::LoweredText) -> e::ImplDecl {
    e::ImplDecl {
        name: ident(value.name, source, text),
        target: value.target.map(|value| ident(value, source, text)),
        methods: value
            .methods
            .iter()
            .map(|value| {
                atom(text, source, value.span);
                e::ImplMethod {
                    exported: value.exported,
                    decl: function(&value.decl, source, text),
                    span: value.span,
                }
            })
            .collect(),
    }
}

fn protocol_decl(
    value: &a::ProtocolDecl,
    source: &str,
    text: &mut e::LoweredText,
) -> e::ProtocolDecl {
    e::ProtocolDecl {
        name: ident(value.name, source, text),
        type_params: value
            .type_params
            .iter()
            .map(|value| ident(*value, source, text))
            .collect(),
        methods: value
            .methods
            .iter()
            .map(|value| function(value, source, text))
            .collect(),
    }
}

fn block(value: &a::Block, source: &str, text: &mut e::LoweredText) -> e::Block {
    atom(text, source, value.span);
    e::Block {
        stmts: value
            .stmts
            .iter()
            .map(|value| stmt(value, source, text))
            .collect(),
        tail: value
            .tail
            .as_ref()
            .map(|value| Rc::new(expr(value, source, text))),
        span: value.span,
    }
}

fn expr(value: &a::Expr, source: &str, text: &mut e::LoweredText) -> e::Expr {
    atom(text, source, value.span);
    e::Expr {
        kind: match &value.kind {
            a::ExprKind::Int => e::ExprKind::Int,
            a::ExprKind::Float => e::ExprKind::Float,
            a::ExprKind::Duration(value) => e::ExprKind::Duration(duration_unit(*value)),
            a::ExprKind::Bool(value) => e::ExprKind::Bool(*value),
            a::ExprKind::Null => e::ExprKind::Null,
            a::ExprKind::Unit => e::ExprKind::Unit,
            a::ExprKind::String(value) => {
                atom(text, source, value.span);
                if let Some(span) = value.tag {
                    atom(text, source, span);
                }
                e::ExprKind::String(e::StringLit {
                    tag: value.tag,
                    multiline: value.multiline,
                    parts: value
                        .parts
                        .iter()
                        .map(|value| match value {
                            a::StringPart::Text(span) => {
                                atom(text, source, *span);
                                e::StringPart::Text(*span)
                            }
                            a::StringPart::Interpolation(value) => {
                                e::StringPart::Interpolation(Box::new(expr(value, source, text)))
                            }
                        })
                        .collect(),
                    span: value.span,
                })
            }
            a::ExprKind::Ident => e::ExprKind::Ident,
            a::ExprKind::Placeholder => e::ExprKind::Placeholder,
            a::ExprKind::Paren(value) => e::ExprKind::Paren(Rc::new(expr(value, source, text))),
            a::ExprKind::Block(value) => e::ExprKind::Block(Rc::new(block(value, source, text))),
            a::ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => e::ExprKind::If {
                cond: Rc::new(expr(cond, source, text)),
                then_block: Rc::new(block(then_block, source, text)),
                else_branch: else_branch
                    .as_ref()
                    .map(|value| Rc::new(expr(value, source, text))),
            },
            a::ExprKind::Match { scrutinee, cases } => e::ExprKind::Match {
                scrutinee: Rc::new(expr(scrutinee, source, text)),
                cases: cases
                    .iter()
                    .map(|value| case_clause(value, source, text))
                    .collect(),
            },
            a::ExprKind::For {
                pattern: value,
                iter,
                body,
            } => e::ExprKind::For {
                pattern: pattern(value, source, text),
                iter: Rc::new(expr(iter, source, text)),
                body: Rc::new(block(body, source, text)),
            },
            a::ExprKind::Loop { label, body } => e::ExprKind::Loop {
                label: label.map(|value| ident(value, source, text)),
                body: Rc::new(block(body, source, text)),
            },
            a::ExprKind::Concurrent {
                timeout,
                arms,
                else_block,
            } => e::ExprKind::Concurrent {
                timeout: timeout
                    .as_ref()
                    .map(|value| Rc::new(expr(value, source, text))),
                arms: arms
                    .iter()
                    .map(|value| {
                        atom(text, source, value.span);
                        e::ConcurrentArm {
                            name: ident(value.name, source, text),
                            value: expr(&value.value, source, text),
                            span: value.span,
                        }
                    })
                    .collect(),
                else_block: else_block
                    .as_ref()
                    .map(|value| Rc::new(block(value, source, text))),
            },
            a::ExprKind::Call {
                callee,
                args,
                type_args,
            } => e::ExprKind::Call {
                callee: Rc::new(expr(callee, source, text)),
                args: args
                    .iter()
                    .map(|value| call_arg(value, source, text))
                    .collect(),
                type_args: type_args
                    .iter()
                    .map(|value| ty_node(value, source, text))
                    .collect(),
            },
            a::ExprKind::Member { object, field } => e::ExprKind::Member {
                object: Rc::new(expr(object, source, text)),
                field: ident(*field, source, text),
            },
            a::ExprKind::Index { object, index } => e::ExprKind::Index {
                object: Rc::new(expr(object, source, text)),
                index: Rc::new(expr(index, source, text)),
            },
            a::ExprKind::OptionalAccess { object, field } => e::ExprKind::OptionalAccess {
                object: Rc::new(expr(object, source, text)),
                field: ident(*field, source, text),
            },
            a::ExprKind::Try(value) => e::ExprKind::Try(Rc::new(expr(value, source, text))),
            a::ExprKind::Unary { op, operand } => e::ExprKind::Unary {
                op: unary_op(*op),
                operand: Rc::new(expr(operand, source, text)),
            },
            a::ExprKind::Binary { op, lhs, rhs } => e::ExprKind::Binary {
                op: binary_op(*op),
                lhs: Rc::new(expr(lhs, source, text)),
                rhs: Rc::new(expr(rhs, source, text)),
            },
            a::ExprKind::Range {
                lo,
                hi,
                inclusive,
                step,
            } => e::ExprKind::Range {
                lo: Rc::new(expr(lo, source, text)),
                hi: Rc::new(expr(hi, source, text)),
                inclusive: *inclusive,
                step: step
                    .as_ref()
                    .map(|value| Rc::new(expr(value, source, text))),
            },
            a::ExprKind::Compose { lhs, rhs } => e::ExprKind::Compose {
                lhs: Rc::new(expr(lhs, source, text)),
                rhs: Rc::new(expr(rhs, source, text)),
            },
            a::ExprKind::Pipe { lhs, rhs } => e::ExprKind::Pipe {
                lhs: Rc::new(expr(lhs, source, text)),
                rhs: match rhs.as_ref() {
                    a::PipeRhs::Expr(value) => e::PipeRhs::Expr(Rc::new(expr(value, source, text))),
                    a::PipeRhs::Field(value) => e::PipeRhs::Field(ident(*value, source, text)),
                },
            },
            a::ExprKind::Lambda { params, body } => {
                text.record_closure_body(value.span, body.span);
                e::ExprKind::Lambda {
                    params: params
                        .iter()
                        .map(|value| {
                            atom(text, source, value.span);
                            e::LambdaParam {
                                name: ident(value.name, source, text),
                                ty: value.ty.as_ref().map(|value| ty_node(value, source, text)),
                                span: value.span,
                            }
                        })
                        .collect(),
                    body: Rc::new(expr(body, source, text)),
                }
            }
            a::ExprKind::RecordLiteral { fields } => e::ExprKind::RecordLiteral {
                fields: fields
                    .iter()
                    .map(|value| field_init(value, source, text))
                    .collect(),
            },
            a::ExprKind::RecordUpdate {
                base,
                spread,
                fields,
            } => e::ExprKind::RecordUpdate {
                base: Rc::new(expr(base, source, text)),
                spread: spread
                    .as_ref()
                    .map(|value| Rc::new(expr(value, source, text))),
                fields: fields
                    .iter()
                    .map(|value| field_init(value, source, text))
                    .collect(),
            },
            a::ExprKind::Array(values) => e::ExprKind::Array(
                values
                    .iter()
                    .map(|value| match value {
                        a::ArrayElement::Expr(value) => {
                            e::ArrayElement::Expr(expr(value, source, text))
                        }
                        a::ArrayElement::Spread(value) => {
                            e::ArrayElement::Spread(expr(value, source, text))
                        }
                    })
                    .collect(),
            ),
            a::ExprKind::SetLiteral(values) => e::ExprKind::SetLiteral(
                values
                    .iter()
                    .map(|value| expr(value, source, text))
                    .collect(),
            ),
            a::ExprKind::MapLiteral(values) => e::ExprKind::MapLiteral(
                values
                    .iter()
                    .map(|(key, value)| (expr(key, source, text), expr(value, source, text)))
                    .collect(),
            ),
            a::ExprKind::Comprehension {
                kind,
                clauses,
                body,
            } => e::ExprKind::Comprehension {
                kind: comp_kind(*kind),
                clauses: clauses
                    .iter()
                    .map(|value| match value {
                        a::CompClause::For {
                            pattern: value,
                            iter,
                        } => e::CompClause::For {
                            pattern: pattern(value, source, text),
                            iter: Rc::new(expr(iter, source, text)),
                        },
                        a::CompClause::If(value) => {
                            e::CompClause::If(Rc::new(expr(value, source, text)))
                        }
                    })
                    .collect(),
                body: match body.as_ref() {
                    a::CompBody::Elem(value) => {
                        e::CompBody::Elem(Rc::new(expr(value, source, text)))
                    }
                    a::CompBody::Entry { key, value } => e::CompBody::Entry {
                        key: Rc::new(expr(key, source, text)),
                        value: Rc::new(expr(value, source, text)),
                    },
                },
            },
        },
        span: value.span,
        call: topaz_hir::lower_call_expr(value, source),
    }
}

fn call_arg(value: &a::CallArg, source: &str, text: &mut e::LoweredText) -> e::CallArg {
    match value {
        a::CallArg::Positional(value) => e::CallArg::Positional(expr(value, source, text)),
        a::CallArg::Spread(value) => e::CallArg::Spread(expr(value, source, text)),
        a::CallArg::Named { name, value } => e::CallArg::Named {
            name: ident(*name, source, text),
            value: expr(value, source, text),
        },
    }
}

fn field_init(value: &a::FieldInit, source: &str, text: &mut e::LoweredText) -> e::FieldInit {
    atom(text, source, value.span);
    e::FieldInit {
        name: ident(value.name, source, text),
        value: expr(&value.value, source, text),
        span: value.span,
    }
}

fn case_clause(value: &a::CaseClause, source: &str, text: &mut e::LoweredText) -> e::CaseClause {
    atom(text, source, value.span);
    e::CaseClause {
        pattern: pattern(&value.pattern, source, text),
        guard: value.guard.as_ref().map(|value| expr(value, source, text)),
        body: match &value.body {
            a::CaseArmBody::Expr(value) => e::CaseArmBody::Expr(expr(value, source, text)),
            a::CaseArmBody::Return { value, span } => {
                atom(text, source, *span);
                e::CaseArmBody::Return {
                    value: value.as_ref().map(|value| expr(value, source, text)),
                    span: *span,
                }
            }
        },
        span: value.span,
    }
}

fn pattern(value: &a::Pattern, source: &str, text: &mut e::LoweredText) -> e::Pattern {
    atom(text, source, value.span);
    e::Pattern {
        kind: match &value.kind {
            a::PatternKind::Or(values) => e::PatternKind::Or(
                values
                    .iter()
                    .map(|value| pattern(value, source, text))
                    .collect(),
            ),
            a::PatternKind::Wildcard => e::PatternKind::Wildcard,
            a::PatternKind::Literal(value) => {
                e::PatternKind::Literal(Rc::new(expr(value, source, text)))
            }
            a::PatternKind::Range { lo, hi, inclusive } => e::PatternKind::Range {
                lo: Rc::new(expr(lo, source, text)),
                hi: Rc::new(expr(hi, source, text)),
                inclusive: *inclusive,
            },
            a::PatternKind::Binding(value) => e::PatternKind::Binding(ident(*value, source, text)),
            a::PatternKind::Typed { name, ty } => e::PatternKind::Typed {
                name: ident(*name, source, text),
                ty: ty_node(ty, source, text),
            },
            a::PatternKind::Constructor { name, args } => e::PatternKind::Constructor {
                name: ident(*name, source, text),
                args: args
                    .iter()
                    .map(|value| pattern(value, source, text))
                    .collect(),
            },
            a::PatternKind::List(values) => e::PatternKind::List(
                values
                    .iter()
                    .map(|value| match value {
                        a::ListPatternElem::Pattern(value) => {
                            e::ListPatternElem::Pattern(pattern(value, source, text))
                        }
                        a::ListPatternElem::Rest(value) => e::ListPatternElem::Rest(
                            value.as_ref().map(|value| pattern(value, source, text)),
                        ),
                    })
                    .collect(),
            ),
            a::PatternKind::Record(values) => e::PatternKind::Record(
                values
                    .iter()
                    .map(|value| record_pattern_field(value, source, text))
                    .collect(),
            ),
            a::PatternKind::NominalRecord { name, fields } => e::PatternKind::NominalRecord {
                name: ident(*name, source, text),
                fields: fields
                    .iter()
                    .map(|value| record_pattern_field(value, source, text))
                    .collect(),
            },
        },
        span: value.span,
    }
}

fn record_pattern_field(
    value: &a::RecordPatternField,
    source: &str,
    text: &mut e::LoweredText,
) -> e::RecordPatternField {
    atom(text, source, value.span);
    e::RecordPatternField {
        name: ident(value.name, source, text),
        pattern: value
            .pattern
            .as_ref()
            .map(|value| pattern(value, source, text)),
        span: value.span,
    }
}

fn ty_node(value: &a::Type, source: &str, text: &mut e::LoweredText) -> e::Type {
    atom(text, source, value.span);
    e::Type {
        kind: match &value.kind {
            a::TypeKind::Named { name, args } => e::TypeKind::Named {
                name: ident(*name, source, text),
                args: args
                    .iter()
                    .map(|value| ty_node(value, source, text))
                    .collect(),
            },
            a::TypeKind::Qualified { ns, name, args } => e::TypeKind::Qualified {
                ns: ident(*ns, source, text),
                name: ident(*name, source, text),
                args: args
                    .iter()
                    .map(|value| ty_node(value, source, text))
                    .collect(),
            },
            a::TypeKind::Literal => e::TypeKind::Literal,
            a::TypeKind::Record(values) => e::TypeKind::Record(
                values
                    .iter()
                    .map(|value| {
                        atom(text, source, value.span);
                        e::FieldType {
                            name: ident(value.name, source, text),
                            ty: ty_node(&value.ty, source, text),
                            span: value.span,
                        }
                    })
                    .collect(),
            ),
            a::TypeKind::Function { params, ret } => e::TypeKind::Function {
                params: params
                    .iter()
                    .map(|value| e::FunctionTypeParam {
                        ty: ty_node(&value.ty, source, text),
                        variadic: value.variadic,
                    })
                    .collect(),
                ret: Box::new(ty_node(ret, source, text)),
            },
            a::TypeKind::Unit => e::TypeKind::Unit,
            a::TypeKind::Union(values) => e::TypeKind::Union(
                values
                    .iter()
                    .map(|value| ty_node(value, source, text))
                    .collect(),
            ),
        },
        span: value.span,
    }
}

fn import_item(value: &a::ImportItem, source: &str, text: &mut e::LoweredText) -> e::ImportItem {
    atom(text, source, value.span);
    atom(text, source, value.path.span);
    e::ImportItem {
        path: e::ModulePath {
            segments: value
                .path
                .segments
                .iter()
                .map(|value| ident(*value, source, text))
                .collect(),
            span: value.path.span,
        },
        kind: match &value.kind {
            a::ImportKind::Namespace { alias } => e::ImportKind::Namespace {
                alias: alias.map(|value| ident(value, source, text)),
            },
            a::ImportKind::Selected { specs } => e::ImportKind::Selected {
                specs: specs
                    .iter()
                    .map(|value| {
                        atom(text, source, value.span);
                        e::ImportSpec {
                            name: ident(value.name, source, text),
                            alias: value.alias.map(|value| ident(value, source, text)),
                            span: value.span,
                        }
                    })
                    .collect(),
            },
        },
        span: value.span,
    }
}

fn assign_op(value: a::AssignOp) -> e::AssignOp {
    match value {
        a::AssignOp::Assign => e::AssignOp::Assign,
        a::AssignOp::Add => e::AssignOp::Add,
        a::AssignOp::Sub => e::AssignOp::Sub,
        a::AssignOp::Mul => e::AssignOp::Mul,
        a::AssignOp::Div => e::AssignOp::Div,
        a::AssignOp::Rem => e::AssignOp::Rem,
        a::AssignOp::Coalesce => e::AssignOp::Coalesce,
    }
}

fn duration_unit(value: topaz_syntax::DurationUnit) -> e::DurationUnit {
    match value {
        topaz_syntax::DurationUnit::Ms => e::DurationUnit::Ms,
        topaz_syntax::DurationUnit::S => e::DurationUnit::S,
        topaz_syntax::DurationUnit::M => e::DurationUnit::M,
    }
}

fn unary_op(value: a::UnaryOp) -> e::UnaryOp {
    match value {
        a::UnaryOp::Plus => e::UnaryOp::Plus,
        a::UnaryOp::Minus => e::UnaryOp::Minus,
        a::UnaryOp::Not => e::UnaryOp::Not,
    }
}

fn binary_op(value: a::BinaryOp) -> e::BinaryOp {
    match value {
        a::BinaryOp::Pow => e::BinaryOp::Pow,
        a::BinaryOp::Mul => e::BinaryOp::Mul,
        a::BinaryOp::Div => e::BinaryOp::Div,
        a::BinaryOp::Rem => e::BinaryOp::Rem,
        a::BinaryOp::Add => e::BinaryOp::Add,
        a::BinaryOp::Sub => e::BinaryOp::Sub,
        a::BinaryOp::Lt => e::BinaryOp::Lt,
        a::BinaryOp::Le => e::BinaryOp::Le,
        a::BinaryOp::Gt => e::BinaryOp::Gt,
        a::BinaryOp::Ge => e::BinaryOp::Ge,
        a::BinaryOp::Eq => e::BinaryOp::Eq,
        a::BinaryOp::Ne => e::BinaryOp::Ne,
        a::BinaryOp::In => e::BinaryOp::In,
        a::BinaryOp::And => e::BinaryOp::And,
        a::BinaryOp::Or => e::BinaryOp::Or,
        a::BinaryOp::Coalesce => e::BinaryOp::Coalesce,
    }
}

fn comp_kind(value: a::CompKind) -> e::CompKind {
    match value {
        a::CompKind::Array => e::CompKind::Array,
        a::CompKind::Set => e::CompKind::Set,
        a::CompKind::Map => e::CompKind::Map,
    }
}
