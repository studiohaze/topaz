//! `check_unit_typed` — the complete checker-owned Typed IR producer.
//!
//! One checker execution returns diagnostics, export/conformance and tooling
//! surfaces, full semantic node/call/capture facts, and conservative
//! representation decisions. A diagnostic-bearing unit exposes no `TypedUnit`.
//! `topaz_hir` owns the closed cross-crate algebra and never depends on the
//! checker.
//!
//! SOUNDNESS (the design's top risk): a value is native ONLY when its type is a
//! CONCRETE scalar (`int`/`float`/`bool`/`()`). EVERYTHING else — any
//! `Unknown`/`Var`/`Skolem`/`Foreign` component, every literal type, union,
//! record, constructor (`Array`/`Map`/`Set`/`Option`/`Result`/`Range`), enum,
//! function, template, `File`, and `JSONValue` — converts to `MonoTy::Boxed`, so
//! the backend can never drop a runtime guard behind an untyped fact.

use std::collections::{BTreeMap, HashMap};
use topaz_diag::{Diagnostic, Span};

use topaz_hir::{
    MonoTy, SemanticConstructor, SemanticField, SemanticLiteral, SemanticPrimitive, SemanticType,
    TypedByteField, TypedByteProjection, TypedByteRecordParam, TypedCall, TypedNode, TypedNodeKind,
    TypedUnit,
};
use topaz_syntax::{LangVersion, ast};

use crate::CheckOutput;
use crate::expr::ExprChecker;
use crate::form::Former;
use crate::ty::{Prim, Type};
use crate::unit::{
    ExportedAlias, ModuleExports, TypedModuleCheck, UnitModule, check_module_typed_with_version,
    module_nominal_identity,
};

/// The result of a typed check: the diagnostics, and — when clean — the typed
/// HIR. When the unit has ANY diagnostic, `typed_hir` is `None` (a value-/type-
/// unsound program never produces native facts).
pub struct CheckedUnit {
    pub diagnostics: Vec<Diagnostic>,
    pub exports: BTreeMap<String, ModuleExports>,
    pub local_aliases: BTreeMap<String, BTreeMap<String, ExportedAlias>>,
    pub conformances: Vec<(String, String)>,
    pub typed_hir: Option<TypedUnit>,
    /// Rich binding/parameter types for tooling surfaces such as LSP hover.
    /// Empty when diagnostics are present, matching `typed_hir`.
    pub hover_types: Vec<HoverType>,
}

/// Convert checker meaning to the closed HIR-owned semantic type algebra.
pub fn semantic_of(ty: &Type) -> SemanticType {
    match ty {
        Type::Prim(value) => SemanticType::Primitive(match value {
            Prim::Int => SemanticPrimitive::Int,
            Prim::Float => SemanticPrimitive::Float,
            Prim::String => SemanticPrimitive::String,
            Prim::Bool => SemanticPrimitive::Bool,
            Prim::Unit => SemanticPrimitive::Unit,
        }),
        Type::Literal(value) => SemanticType::Literal(match value {
            crate::ty::Lit::Str(value) => SemanticLiteral::String(value.clone()),
            crate::ty::Lit::Int(value) => SemanticLiteral::Int(*value),
            crate::ty::Lit::Float(value) => SemanticLiteral::Float(value.clone()),
            crate::ty::Lit::Bool(value) => SemanticLiteral::Bool(*value),
            crate::ty::Lit::Null => SemanticLiteral::Null,
        }),
        Type::Union(values) => SemanticType::Union(values.iter().map(semantic_of).collect()),
        Type::Record(fields) => SemanticType::Record(
            fields
                .iter()
                .map(|(name, ty)| SemanticField {
                    name: name.clone(),
                    ty: semantic_of(ty),
                })
                .collect(),
        ),
        Type::Ctor(constructor, arguments) => SemanticType::Constructor {
            constructor: match constructor {
                crate::ty::Ctor::Array => SemanticConstructor::Array,
                crate::ty::Ctor::Map => SemanticConstructor::Map,
                crate::ty::Ctor::Set => SemanticConstructor::Set,
                crate::ty::Ctor::Option => SemanticConstructor::Option,
                crate::ty::Ctor::Result => SemanticConstructor::Result,
                crate::ty::Ctor::Range => SemanticConstructor::Range,
            },
            arguments: arguments.iter().map(semantic_of).collect(),
        },
        Type::Func {
            params,
            variadic,
            ret,
        } => SemanticType::Function {
            parameters: params.iter().map(semantic_of).collect(),
            variadic: variadic.as_deref().map(semantic_of).map(Box::new),
            result: Box::new(semantic_of(ret)),
        },
        Type::Foreign { name, args } => SemanticType::Foreign {
            identity: name.clone(),
            arguments: args.iter().map(semantic_of).collect(),
        },
        Type::Skolem { name, origin, .. } => SemanticType::Rigid {
            name: name.clone(),
            origin: origin.clone(),
        },
        Type::Template => SemanticType::Template,
        Type::File => SemanticType::File,
        Type::JsonValue => SemanticType::JsonValue,
        Type::Bytes => SemanticType::Bytes,
        Type::ByteBuffer => SemanticType::ByteBuffer,
        Type::Path => SemanticType::Path,
        Type::Regex => SemanticType::Regex,
        Type::Match => SemanticType::Match,
        Type::TomlValue => SemanticType::TomlValue,
        Type::Url => SemanticType::Url,
        Type::Date => SemanticType::Date,
        Type::BigInt => SemanticType::BigInt,
        Type::Decimal => SemanticType::Decimal,
        Type::RoundingMode => SemanticType::RoundingMode,
        Type::Enum { base, args } => SemanticType::Enum {
            identity: base.clone(),
            arguments: args.iter().map(semantic_of).collect(),
        },
        Type::NominalRecord { base, args } => SemanticType::NominalRecord {
            identity: base.clone(),
            arguments: args.iter().map(semantic_of).collect(),
        },
        Type::Newtype { base, args } => SemanticType::Newtype {
            identity: base.clone(),
            arguments: args.iter().map(semantic_of).collect(),
        },
        Type::Unknown => SemanticType::Unknown,
        Type::Var(_) => SemanticType::InferenceVariable,
    }
}

/// One hoverable binding/parameter span and its checker-level type rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverType {
    pub name: String,
    pub span: Span,
    pub ty: String,
    pub raw_ty: Type,
}

/// Convert the checker's rich `Type` to the codegen-facing [`MonoTy`].
///
/// The four concrete scalars become bare native values. Exact `Bytes` and
/// `ByteBuffer` become boxed handle facts; everything else is `Boxed`. A
/// `Type::Var`/`Unknown`/`Skolem`/`Foreign` never creates either a scalar or a
/// handle fact.
pub fn mono_of(ty: &Type) -> MonoTy {
    match ty {
        Type::Prim(Prim::Int) => MonoTy::I64,
        Type::Prim(Prim::Float) => MonoTy::F64,
        Type::Prim(Prim::Bool) => MonoTy::Bool,
        Type::Prim(Prim::Unit) => MonoTy::Unit,
        // Exact byte values retain the boxed runtime carrier but expose a
        // checker-owned handle fact to the bounded native hot path.
        Type::Bytes => MonoTy::BytesHandle,
        Type::ByteBuffer => MonoTy::ByteBufferHandle,
        // String is a §22 boxed `Value::Str`; every remaining literal, union,
        // aggregate, nominal, stdlib value, and unknown type is boxed.
        Type::Prim(Prim::String)
        | Type::Literal(_)
        | Type::Union(_)
        | Type::Record(_)
        | Type::Ctor(_, _)
        | Type::Func { .. }
        | Type::Foreign { .. }
        | Type::Skolem { .. }
        | Type::Template
        | Type::File
        | Type::JsonValue
        | Type::Path
        | Type::Regex
        | Type::Match
        | Type::TomlValue
        | Type::Url
        | Type::Date
        | Type::BigInt
        | Type::Decimal
        | Type::RoundingMode
        | Type::Enum { .. }
        | Type::NominalRecord { .. }
        | Type::Newtype { .. }
        | Type::Unknown
        | Type::Var(_) => MonoTy::Boxed,
    }
}

#[derive(Default)]
struct ByteFacts {
    record_params: Vec<TypedByteRecordParam>,
    projections: Vec<TypedByteProjection>,
}

/// Derive the deliberately narrow read-only record bridge from clean checker
/// bindings plus direct source declarations. This is part of the checker→HIR
/// boundary, not emitter inference: the caller only installs these facts when
/// the complete check is diagnostic-free.
///
/// The admitted syntax is intentionally smaller than the language:
/// top-level, non-generic functions; a direct annotation naming a
/// same-module, non-generic record; a direct declared Bytes/ByteBuffer field;
/// and a body-level simple `let local = parameter.field`. Qualified names,
/// aliases, nested projections, destructuring, and nested-block bindings do not
/// produce a fact.
fn collect_program_byte_facts(
    module: &str,
    defining_module: &str,
    version: LangVersion,
    src: &str,
    program: &ast::Program,
    typed_locals: &[(String, Span, Type)],
) -> ByteFacts {
    let local_monos = typed_locals
        .iter()
        .map(|(name, span, ty)| ((span.file, name.clone(), span.lo, span.hi), mono_of(ty)))
        .collect::<HashMap<_, _>>();

    let mut records: HashMap<String, Vec<TypedByteField>> = HashMap::new();
    for item in &program.items {
        let item = unwrap_export(item);
        let ast::StmtKind::Record(record) = &item.kind else {
            continue;
        };
        if !record.type_params.is_empty() {
            continue;
        }
        let mut fields = Vec::new();
        for field in &record.fields {
            let ast::TypeKind::Named { name, args } = &field.ty.kind else {
                continue;
            };
            if !args.is_empty() {
                continue;
            }
            let mono = match source_text(src, name.span) {
                "Bytes" => MonoTy::BytesHandle,
                "ByteBuffer" => MonoTy::ByteBufferHandle,
                _ => continue,
            };
            fields.push(TypedByteField {
                name: source_text(src, field.name.span).to_string(),
                mono,
            });
        }
        if !fields.is_empty() {
            records.insert(source_text(src, record.name.span).to_string(), fields);
        }
    }

    let mut out = ByteFacts::default();
    for item in &program.items {
        let item = unwrap_export(item);
        let ast::StmtKind::Function(function) = &item.kind else {
            continue;
        };
        if !function.type_params.is_empty() {
            continue;
        }
        let function_span = function.name.span;
        let mut eligible_params: HashMap<String, TypedByteRecordParam> = HashMap::new();
        for param in &function.params {
            if param.variadic || param.default.is_some() {
                continue;
            }
            let ast::TypeKind::Named { name, args } = &param.ty.kind else {
                continue;
            };
            if !args.is_empty() {
                continue;
            }
            let record = source_text(src, name.span);
            let Some(fields) = records.get(record) else {
                continue;
            };
            let param_name = source_text(src, param.name.span).to_string();
            if local_monos.get(&(
                param.name.span.file,
                param_name.clone(),
                param.name.span.lo,
                param.name.span.hi,
            )) != Some(&MonoTy::Boxed)
            {
                continue;
            }
            let fact = TypedByteRecordParam {
                module: module.to_string(),
                function_span,
                name: param_name.clone(),
                span: param.name.span,
                declaration_identity: if version >= LangVersion::V5_20 {
                    module_nominal_identity(defining_module, record)
                } else {
                    record.to_string()
                },
                fields: fields.clone(),
            };
            eligible_params.insert(param_name, fact.clone());
            out.record_params.push(fact);
        }

        if eligible_params.is_empty() {
            continue;
        }
        for statement in &function.body.stmts {
            let ast::StmtKind::Let { pattern, value, .. } = &statement.kind else {
                continue;
            };
            let local_ident = match &pattern.kind {
                ast::PatternKind::Binding(name) | ast::PatternKind::Typed { name, .. } => name,
                _ => continue,
            };
            let ast::ExprKind::Member { object, field } = &value.kind else {
                continue;
            };
            if !matches!(object.kind, ast::ExprKind::Ident) {
                continue;
            }
            let receiver_name = source_text(src, object.span);
            let Some(param) = eligible_params.get(receiver_name) else {
                continue;
            };
            let field_name = source_text(src, field.span);
            let Some(field_fact) = param.fields.iter().find(|fact| fact.name == field_name) else {
                continue;
            };
            let local_name = source_text(src, local_ident.span).to_string();
            let Some(local_mono) = local_monos.get(&(
                local_ident.span.file,
                local_name.clone(),
                local_ident.span.lo,
                local_ident.span.hi,
            )) else {
                continue;
            };
            if *local_mono != field_fact.mono {
                continue;
            }
            out.projections.push(TypedByteProjection {
                module: module.to_string(),
                function_span,
                receiver_name: param.name.clone(),
                receiver_span: param.span,
                field: field_name.to_string(),
                expression_span: value.span,
                local_name,
                local_span: local_ident.span,
                mono: *local_mono,
            });
        }
    }
    out
}

fn unwrap_export(mut statement: &ast::Stmt) -> &ast::Stmt {
    while let ast::StmtKind::Export(inner) = &statement.kind {
        statement = inner;
    }
    statement
}

fn source_text(src: &str, span: Span) -> &str {
    &src[span.lo as usize..span.hi as usize]
}

fn install_byte_facts(checked: &mut CheckedUnit, mut facts: ByteFacts) {
    let Some(typed) = checked.typed_hir.as_mut() else {
        return;
    };
    facts.record_params.sort_by_key(|fact| {
        (
            fact.module.clone(),
            fact.function_span.file.0,
            fact.function_span.lo,
            fact.span.lo,
        )
    });
    facts.projections.sort_by_key(|fact| {
        (
            fact.module.clone(),
            fact.function_span.file.0,
            fact.function_span.lo,
            fact.local_span.lo,
        )
    });
    for fact in facts.record_params {
        typed.push_byte_record_param(fact);
    }
    for fact in facts.projections {
        typed.push_byte_projection(fact);
    }
}

/// Single-program typed check (the non-module entry, mirroring `check_program`).
pub fn check_program_typed(src: &str, program: &ast::Program) -> CheckedUnit {
    check_program_typed_with_version(src, program, LangVersion::CURRENT)
}

pub fn check_program_typed_with_version(
    src: &str,
    program: &ast::Program,
    version: LangVersion,
) -> CheckedUnit {
    let mut former = Former::with_version(src, program, version);
    former.validate_aliases();
    let mut checker = ExprChecker::new(former);
    checker.enable_typed_locals();
    checker.check_items(&program.items);
    let typed_locals = checker.take_typed_locals();
    let typed_nodes = checker.take_typed_nodes();
    let typed_call_callees = checker.take_typed_call_callees();
    let byte_facts =
        collect_program_byte_facts("<program>", "", version, src, program, &typed_locals);
    let mut conformances: Vec<(String, String)> = checker
        .former
        .conformances()
        .map(|(protocol, type_id)| (protocol.to_string(), type_id.to_string()))
        .collect();
    conformances.sort();
    let output = CheckOutput {
        diagnostics: checker.former.diagnostics,
        exports: BTreeMap::new(),
        local_aliases: BTreeMap::new(),
        conformances,
    };
    let contains_concurrent = items_contain_concurrent(&program.items);
    let mut checked = finish(
        output,
        typed_locals,
        typed_nodes,
        Vec::new(),
        typed_call_callees
            .into_iter()
            .map(|(span, ty)| ("<program>".to_string(), span, ty))
            .collect(),
        &[UnitModule {
            identity: "<program>".to_string(),
            is_entry: true,
            is_extern: false,
            is_generated_std: false,
            extern_replay_error: None,
            src,
            program,
        }],
        contains_concurrent,
    );
    install_byte_facts(&mut checked, byte_facts);
    checked
}

/// Module-aware typed check (mirrors `check_unit`): checks every module in
/// dependency order, collecting typed locals across all of them, and produces
/// the typed HIR only when the WHOLE unit is clean.
pub fn check_unit_typed(modules: &[UnitModule<'_>]) -> CheckedUnit {
    check_unit_typed_with_version(modules, LangVersion::CURRENT)
}

pub fn check_unit_typed_with_version(
    modules: &[UnitModule<'_>],
    version: LangVersion,
) -> CheckedUnit {
    let TypedModuleCheck {
        output,
        locals: typed_locals,
        nodes: typed_nodes,
        call_targets: typed_call_targets,
        call_callees: typed_call_callees,
    } = check_module_typed_with_version(modules, version);
    let mut byte_facts = ByteFacts::default();
    for module in modules.iter().filter(|module| !module.is_extern) {
        let defining_module = if module.is_entry {
            ""
        } else {
            module.identity.as_str()
        };
        let mut module_facts = collect_program_byte_facts(
            &module.identity,
            defining_module,
            version,
            module.src,
            module.program,
            &typed_locals,
        );
        byte_facts
            .record_params
            .append(&mut module_facts.record_params);
        byte_facts.projections.append(&mut module_facts.projections);
    }
    // The `concurrent` fact is UNIT-WIDE: ANY module bearing a `concurrent`
    // disables native loop-checkpoint elision for the whole unit (a loop in one
    // module could be driven from a `concurrent` arm in another).
    let contains_concurrent = modules
        .iter()
        .any(|m| items_contain_concurrent(&m.program.items));
    let mut checked = finish(
        output,
        typed_locals,
        typed_nodes,
        typed_call_targets,
        typed_call_callees,
        modules,
        contains_concurrent,
    );
    install_byte_facts(&mut checked, byte_facts);
    checked
}

/// Build the [`CheckedUnit`]: convert the collected locals to a `TypedUnit` ONLY
/// when there are no diagnostics; otherwise hand back the diagnostics with no
/// typed HIR.
fn finish(
    output: CheckOutput,
    typed_locals: Vec<(String, topaz_diag::Span, Type)>,
    typed_nodes: Vec<(TypedNodeKind, topaz_diag::Span, Type)>,
    typed_call_targets: Vec<(String, topaz_diag::Span, String)>,
    typed_call_callees: Vec<(String, topaz_diag::Span, Type)>,
    modules: &[UnitModule<'_>],
    contains_concurrent: bool,
) -> CheckedUnit {
    let CheckOutput {
        diagnostics,
        exports,
        local_aliases,
        conformances,
    } = output;
    if diagnostics.is_empty() {
        let mut typed = TypedUnit::new();
        let mut hover_types = Vec::with_capacity(typed_locals.len());
        let module_by_file = modules
            .iter()
            .map(|module| (module.program.span.file, module.identity.clone()))
            .collect::<HashMap<_, _>>();
        for (name, span, ty) in typed_locals {
            hover_types.push(HoverType {
                name: name.clone(),
                span,
                ty: ty.to_string(),
                raw_ty: ty.clone(),
            });
            typed.push_local(name, span, mono_of(&ty));
            typed.push_node(TypedNode {
                module: module_by_file
                    .get(&span.file)
                    .cloned()
                    .unwrap_or_else(|| "<unknown>".to_string()),
                kind: TypedNodeKind::Binding,
                span,
                ty: semantic_of(&ty),
                ambient: ty.has_unknown(),
            });
        }
        for (kind, span, ty) in typed_nodes {
            typed.push_node(TypedNode {
                module: module_by_file
                    .get(&span.file)
                    .cloned()
                    .unwrap_or_else(|| "<unknown>".to_string()),
                kind,
                span,
                ty: semantic_of(&ty),
                ambient: ty.has_unknown(),
            });
        }
        typed.nodes.sort_by_key(|fact| {
            (
                fact.module.clone(),
                fact.span.file.0,
                fact.span.lo,
                fact.span.hi,
                fact.kind,
            )
        });
        typed.nodes.dedup_by(|left, right| {
            left.module == right.module && left.kind == right.kind && left.span == right.span
        });
        let expression_types = typed
            .nodes
            .iter()
            .filter(|fact| fact.kind == TypedNodeKind::Expression)
            .map(|fact| {
                (
                    (fact.span.file.0, fact.span.lo, fact.span.hi),
                    fact.ty.clone(),
                )
            })
            .collect::<HashMap<_, _>>();
        let mut call_targets = HashMap::new();
        for (module, span, target) in typed_call_targets {
            call_targets
                .entry(module)
                .or_insert_with(HashMap::new)
                .entry((span.file.0, span.lo, span.hi))
                .or_insert(target);
        }
        let mut call_callees = HashMap::new();
        for (module, span, ty) in typed_call_callees {
            call_callees
                .entry(module)
                .or_insert_with(HashMap::new)
                .entry((span.file.0, span.lo, span.hi))
                .or_insert_with(|| semantic_of(&ty));
        }
        for module in modules.iter().filter(|module| !module.is_extern) {
            for plan in topaz_hir::collect_call_plans(module.program, module.src) {
                let result_type = expression_types
                    .get(&(plan.span.file.0, plan.span.lo, plan.span.hi))
                    .cloned()
                    .unwrap_or(SemanticType::Unknown);
                let callee_type = call_callees
                    .get(&module.identity)
                    .and_then(|callees| {
                        callees.get(&(plan.span.file.0, plan.span.lo, plan.span.hi))
                    })
                    .or_else(|| {
                        expression_types.get(&(
                            plan.callee_span.file.0,
                            plan.callee_span.lo,
                            plan.callee_span.hi,
                        ))
                    })
                    .cloned()
                    .unwrap_or(SemanticType::Unknown);
                let ambient = callee_type.has_hole() || result_type.has_hole();
                typed.push_call(TypedCall {
                    module: module.identity.clone(),
                    span: plan.span,
                    callee_span: plan.callee_span,
                    callee_type,
                    result_type,
                    target_identity: call_targets
                        .get(&module.identity)
                        .and_then(|targets| {
                            targets.get(&(plan.span.file.0, plan.span.lo, plan.span.hi))
                        })
                        .cloned(),
                    ambient,
                    plan,
                });
            }
        }
        typed.calls.sort_by_key(|fact| {
            (
                fact.module.clone(),
                fact.span.file.0,
                fact.span.lo,
                fact.span.hi,
            )
        });
        typed.contains_concurrent = contains_concurrent;
        CheckedUnit {
            diagnostics,
            exports,
            local_aliases,
            conformances,
            typed_hir: Some(typed),
            hover_types,
        }
    } else {
        CheckedUnit {
            diagnostics,
            exports,
            local_aliases,
            conformances,
            typed_hir: None,
            hover_types: Vec::new(),
        }
    }
}

// --- `concurrent`-presence walk (the native checkpoint-elision fact) ---
//
// An EXHAUSTIVE AST walk: does any statement/expression ANYWHERE in the unit
// (including delayed positions — lambda/function/defer bodies, match arms,
// branches, loop bodies, default parameters) contain an `ExprKind::Concurrent`?
// CONSERVATIVE by construction (any sighting ⇒ `true` ⇒ native KEEPS every loop
// checkpoint), and the `match` arms are exhaustive (no wildcard) so a NEW
// expression/statement kind cannot silently slip a hidden `concurrent` past the
// walk — the compiler forces this code to handle it. Soundness depends on that:
// a MISSED `concurrent` would let native wrongly elide a checkpoint a concurrent
// arm needs.

fn items_contain_concurrent(items: &[ast::Stmt]) -> bool {
    items.iter().any(stmt_has_concurrent)
}

fn block_has_concurrent(block: &ast::Block) -> bool {
    block.stmts.iter().any(stmt_has_concurrent)
        || block.tail.as_deref().is_some_and(expr_has_concurrent)
}

fn stmt_has_concurrent(stmt: &ast::Stmt) -> bool {
    use ast::StmtKind as S;
    match &stmt.kind {
        // A newtype's base is a TYPE (no expression), so it cannot host `concurrent`.
        // A protocol declares signatures only (empty method bodies) — nothing to host.
        S::Import(_)
        | S::TypeAlias(_)
        | S::Enum(_)
        | S::Newtype(_)
        | S::Protocol(_)
        | S::Continue { .. } => false,
        // A `break <value>` value can host a `concurrent`.
        S::Break { value, .. } => value.as_ref().is_some_and(expr_has_concurrent),
        // A record field DEFAULT can carry an expression (so a `concurrent` could
        // hide there).
        S::Record(decl) => decl
            .fields
            .iter()
            .any(|f| f.default.as_ref().is_some_and(|e| expr_has_concurrent(e))),
        S::Export(inner) => stmt_has_concurrent(inner),
        S::Function(decl) => {
            decl.params
                .iter()
                .any(|p| p.default.as_ref().is_some_and(expr_has_concurrent))
                || block_has_concurrent(&decl.body)
        }
        // §4 (v5.4) an impl method body can host a `concurrent` (or a param default).
        S::Impl(decl) => decl.methods.iter().any(|m| {
            m.decl
                .params
                .iter()
                .any(|p| p.default.as_ref().is_some_and(expr_has_concurrent))
                || block_has_concurrent(&m.decl.body)
        }),
        // A `let` PATTERN can carry expressions (a literal/range subpattern), so a
        // `concurrent` could hide there as well as in the value.
        S::Let { pattern, value, .. } => {
            pattern_has_concurrent(pattern) || expr_has_concurrent(value)
        }
        S::Using { value, body, .. } => expr_has_concurrent(value) || block_has_concurrent(body),
        S::Const { value, .. } => expr_has_concurrent(value),
        S::Assign { target, value, .. } => {
            expr_has_concurrent(target) || expr_has_concurrent(value)
        }
        S::Return(opt) => opt.as_ref().is_some_and(expr_has_concurrent),
        S::Defer(e) => expr_has_concurrent(e),
        S::While { cond, body } => expr_has_concurrent(cond) || block_has_concurrent(body),
        S::Expr(e) => expr_has_concurrent(e),
    }
}

fn expr_has_concurrent(expr: &ast::Expr) -> bool {
    use ast::ExprKind as E;
    match &expr.kind {
        // The one we are hunting.
        E::Concurrent { .. } => true,
        // Leaves — no sub-expression.
        E::Int
        | E::Float
        | E::Duration(_)
        | E::Bool(_)
        | E::Null
        | E::Unit
        | E::Ident
        | E::Placeholder => false,
        E::String(lit) => lit.parts.iter().any(|p| match p {
            ast::StringPart::Text(_) => false,
            ast::StringPart::Interpolation(e) => expr_has_concurrent(e),
        }),
        E::Paren(inner) | E::Try(inner) => expr_has_concurrent(inner),
        E::Block(block) => block_has_concurrent(block),
        E::If {
            cond,
            then_block,
            else_branch,
        } => {
            expr_has_concurrent(cond)
                || block_has_concurrent(then_block)
                || else_branch.as_deref().is_some_and(expr_has_concurrent)
        }
        E::Match { scrutinee, cases } => {
            expr_has_concurrent(scrutinee)
                || cases.iter().any(|c| {
                    // A case PATTERN can carry expressions (a literal/range
                    // subpattern) — descend it too, not just the guard/body.
                    pattern_has_concurrent(&c.pattern)
                        || c.guard.as_ref().is_some_and(expr_has_concurrent)
                        || match &c.body {
                            ast::CaseArmBody::Expr(e) => expr_has_concurrent(e),
                            ast::CaseArmBody::Return { value, .. } => {
                                value.as_ref().is_some_and(expr_has_concurrent)
                            }
                        }
                })
        }
        E::For {
            pattern,
            iter,
            body,
        } => {
            pattern_has_concurrent(pattern)
                || expr_has_concurrent(iter)
                || block_has_concurrent(body)
        }
        // A `loop` body can host a `concurrent`.
        E::Loop { body, .. } => block_has_concurrent(body),
        E::Call { callee, args, .. } => {
            expr_has_concurrent(callee)
                || args.iter().any(|a| match a {
                    ast::CallArg::Positional(e)
                    | ast::CallArg::Spread(e)
                    | ast::CallArg::Named { value: e, .. } => expr_has_concurrent(e),
                })
        }
        E::Member { object, .. } | E::OptionalAccess { object, .. } => expr_has_concurrent(object),
        E::Index { object, index } => expr_has_concurrent(object) || expr_has_concurrent(index),
        E::Unary { operand, .. } => expr_has_concurrent(operand),
        E::Binary { lhs, rhs, .. } | E::Compose { lhs, rhs } => {
            expr_has_concurrent(lhs) || expr_has_concurrent(rhs)
        }
        E::Range { lo, hi, step, .. } => {
            expr_has_concurrent(lo)
                || expr_has_concurrent(hi)
                || step.as_deref().is_some_and(expr_has_concurrent)
        }
        E::Pipe { lhs, rhs } => {
            expr_has_concurrent(lhs)
                || match rhs.as_ref() {
                    ast::PipeRhs::Expr(e) => expr_has_concurrent(e),
                    ast::PipeRhs::Field(_) => false,
                }
        }
        // A lambda's params carry no expression (only a name + optional type), so
        // only the body can hold a `concurrent`.
        E::Lambda { params: _, body } => expr_has_concurrent(body),
        E::RecordLiteral { fields } => fields.iter().any(|f| expr_has_concurrent(&f.value)),
        E::RecordUpdate {
            base,
            spread,
            fields,
        } => {
            expr_has_concurrent(base)
                || spread.as_ref().is_some_and(|s| expr_has_concurrent(s))
                || fields.iter().any(|f| expr_has_concurrent(&f.value))
        }
        E::Array(elems) => elems.iter().any(|el| match el {
            ast::ArrayElement::Expr(e) | ast::ArrayElement::Spread(e) => expr_has_concurrent(e),
        }),
        E::SetLiteral(elems) => elems.iter().any(expr_has_concurrent),
        E::MapLiteral(entries) => entries
            .iter()
            .any(|(k, v)| expr_has_concurrent(k) || expr_has_concurrent(v)),
        E::Comprehension { clauses, body, .. } => {
            clauses.iter().any(|c| match c {
                ast::CompClause::For { pattern, iter } => {
                    pattern_has_concurrent(pattern) || expr_has_concurrent(iter)
                }
                ast::CompClause::If(cond) => expr_has_concurrent(cond),
            }) || match body.as_ref() {
                ast::CompBody::Elem(e) => expr_has_concurrent(e),
                ast::CompBody::Entry { key, value } => {
                    expr_has_concurrent(key) || expr_has_concurrent(value)
                }
            }
        }
    }
}

/// Whether a PATTERN contains an `ExprKind::Concurrent` anywhere. A pattern is
/// mostly structural, but `PatternKind::Literal` and `PatternKind::Range` carry
/// `Expr` endpoints (a range pattern's bounds are expressions), so a `concurrent`
/// CAN hide inside a pattern (e.g. `case (concurrent { a: 1 }).a .. 3 => …`).
/// The `contains_concurrent` fact must see it, or native would wrongly elide a
/// loop checkpoint a concurrent arm needs. EXHAUSTIVE (no wildcard) so a new
/// `PatternKind` fails to compile rather than silently skipping.
fn pattern_has_concurrent(pattern: &ast::Pattern) -> bool {
    use ast::PatternKind as P;
    match &pattern.kind {
        // The leaf patterns carry no expression.
        P::Wildcard | P::Binding(_) | P::Typed { .. } => false,
        // A literal pattern's literal IS an expression.
        P::Literal(expr) => expr_has_concurrent(expr),
        // A range pattern's endpoints are expressions (const-folded later, but
        // syntactically arbitrary here).
        P::Range { lo, hi, .. } => expr_has_concurrent(lo) || expr_has_concurrent(hi),
        // Composite patterns: descend every sub-pattern.
        P::Or(alts) => alts.iter().any(pattern_has_concurrent),
        P::Constructor { args, .. } => args.iter().any(pattern_has_concurrent),
        P::List(elems) => elems.iter().any(|el| match el {
            ast::ListPatternElem::Pattern(p) => pattern_has_concurrent(p),
            ast::ListPatternElem::Rest(opt) => opt.as_ref().is_some_and(pattern_has_concurrent),
        }),
        P::Record(fields) | P::NominalRecord { fields, .. } => fields
            .iter()
            .any(|f| f.pattern.as_ref().is_some_and(pattern_has_concurrent)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use topaz_diag::FileId;
    use topaz_hir::MonoTy;
    use topaz_parser::{ParseOptions, parse_with_options};
    use topaz_syntax::LangVersion;

    /// Parse + typed-check a single program (v5.3, so enums parse), asserting it
    /// is clean and returning its typed HIR.
    fn typed(src: &str) -> TypedUnit {
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::V5_3,
            },
        );
        assert!(
            out.diagnostics.is_empty(),
            "parse failed: {:?}",
            out.diagnostics
        );
        let checked = check_program_typed(src, &out.program);
        assert!(
            checked.diagnostics.is_empty(),
            "check failed: {:?}",
            checked.diagnostics
        );
        checked.typed_hir.expect("clean check yields typed HIR")
    }

    fn typed_current(src: &str) -> TypedUnit {
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::CURRENT,
            },
        );
        assert!(
            out.diagnostics.is_empty(),
            "parse failed: {:?}",
            out.diagnostics
        );
        let checked = check_program_typed(src, &out.program);
        assert!(
            checked.diagnostics.is_empty(),
            "check failed: {:?}",
            checked.diagnostics
        );
        checked.typed_hir.expect("clean check yields typed HIR")
    }

    #[test]
    fn scalar_locals_lower_to_native_repr() {
        let hir = typed("let n = 1\nlet x = 1.5\nlet b = true\nlet u = ()\n");
        assert_eq!(hir.local_mono("n"), Some(MonoTy::I64));
        assert_eq!(hir.local_mono("x"), Some(MonoTy::F64));
        assert_eq!(hir.local_mono("b"), Some(MonoTy::Bool));
        assert_eq!(hir.local_mono("u"), Some(MonoTy::Unit));
    }

    #[test]
    fn annotated_scalar_locals_lower_to_native_repr() {
        let hir = typed("let n: int = 1\nlet x: float = 1.5\nlet b: bool = false\n");
        assert_eq!(hir.local_mono("n"), Some(MonoTy::I64));
        assert_eq!(hir.local_mono("x"), Some(MonoTy::F64));
        assert_eq!(hir.local_mono("b"), Some(MonoTy::Bool));
    }

    #[test]
    fn for_loop_bindings_lower_to_native_repr() {
        let hir = typed("let mut s = 0\nfor x in [1, 2, 3] { s += x }\ns\n");
        assert_eq!(hir.local_mono("x"), Some(MonoTy::I64));
    }

    #[test]
    fn string_and_aggregate_locals_are_boxed() {
        let hir = typed("let s = \"hi\"\nlet a = [1, 2, 3]\nlet r = { x: 1 }\n");
        assert_eq!(hir.local_mono("s"), Some(MonoTy::Boxed));
        assert_eq!(hir.local_mono("a"), Some(MonoTy::Boxed));
        assert_eq!(hir.local_mono("r"), Some(MonoTy::Boxed));
    }

    #[test]
    fn enum_local_is_boxed() {
        let hir = typed("enum Color { Red, Green }\nlet c: Color = Color.Red\n");
        assert_eq!(hir.local_mono("c"), Some(MonoTy::Boxed));
    }

    #[test]
    fn option_and_result_locals_are_boxed() {
        let hir = typed("let o = Some(1)\nlet e: Result<int, string> = Ok(1)\n");
        assert_eq!(hir.local_mono("o"), Some(MonoTy::Boxed));
        assert_eq!(hir.local_mono("e"), Some(MonoTy::Boxed));
    }

    #[test]
    fn branch_solutions_close_typed_node_and_call_facts() {
        fn contains_inference_variable(ty: &SemanticType) -> bool {
            match ty {
                SemanticType::InferenceVariable => true,
                SemanticType::Union(values) => values.iter().any(contains_inference_variable),
                SemanticType::Record(fields) => fields
                    .iter()
                    .any(|field| contains_inference_variable(&field.ty)),
                SemanticType::Constructor { arguments, .. }
                | SemanticType::Foreign { arguments, .. }
                | SemanticType::Enum { arguments, .. }
                | SemanticType::NominalRecord { arguments, .. }
                | SemanticType::Newtype { arguments, .. } => {
                    arguments.iter().any(contains_inference_variable)
                }
                SemanticType::Function {
                    parameters,
                    variadic,
                    result,
                } => {
                    parameters.iter().any(contains_inference_variable)
                        || variadic.as_deref().is_some_and(contains_inference_variable)
                        || contains_inference_variable(result)
                }
                _ => false,
            }
        }

        let hir = typed_current(
            "function choose(flag: bool) -> Option<int> {\n  if flag { Some(1) } else { None }\n}\nfunction recover(flag: bool) -> Result<int, string> {\n  if flag { Ok(1) } else { Err(\"no\") }\n}\nlet no: Option<int> = None\nno == None\nchoose(true)\nrecover(false)\n",
        );
        assert!(
            hir.nodes
                .iter()
                .all(|fact| !contains_inference_variable(&fact.ty))
        );
        assert!(hir.calls.iter().all(|fact| {
            !contains_inference_variable(&fact.callee_type)
                && !contains_inference_variable(&fact.result_type)
        }));
    }

    #[test]
    fn protocol_static_call_retains_concrete_callee_and_target() {
        let src = "record User derives Show { name: string }\nfunction render<T: Show>(value: T) -> string {\n  Show.show(value)\n}\nlet out: string = render(User { name: \"Ada\" })\nprint(out)\n";
        let parsed = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::CURRENT,
            },
        );
        assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
        let modules = [UnitModule {
            identity: "__entry__".to_string(),
            is_entry: true,
            is_extern: false,
            is_generated_std: false,
            extern_replay_error: None,
            src,
            program: &parsed.program,
        }];
        let checked = check_unit_typed_with_version(&modules, LangVersion::CURRENT);
        assert!(checked.diagnostics.is_empty(), "{:#?}", checked.diagnostics);
        let hir = checked.typed_hir.expect("clean unit yields typed HIR");
        let call = hir
            .calls
            .iter()
            .find(|call| call.target_identity.as_deref() == Some("builtin::Show"))
            .expect("Show.show call retains its protocol dispatch identity");
        assert!(!call.ambient);
        assert!(matches!(
            &call.callee_type,
            SemanticType::Function {
                parameters,
                variadic: None,
                result,
            } if parameters.len() == 1
                && matches!(result.as_ref(), SemanticType::Primitive(SemanticPrimitive::String))
        ));
        assert!(hir.nodes.iter().any(|node| {
            node.kind == TypedNodeKind::Expression
                && node.span == call.callee_span
                && node.ty == call.callee_type
        }));
    }

    #[test]
    fn concrete_params_are_native_and_generic_params_are_boxed() {
        // `n: int` is native; the generic `t: T` is a skolem inside the body ->
        // boxed (the soundness rule: no native behind a non-concrete type).
        let hir = typed("function f<T>(n: int, t: T) -> int { n }\n");
        assert_eq!(hir.local_mono("n"), Some(MonoTy::I64));
        assert_eq!(hir.local_mono("t"), Some(MonoTy::Boxed));
    }

    #[test]
    fn a_program_with_diagnostics_yields_no_typed_hir() {
        let src = "let n: int = \"not an int\"\n";
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::V5_2,
            },
        );
        assert!(
            out.diagnostics.is_empty(),
            "parse failed: {:?}",
            out.diagnostics
        );
        let checked = check_program_typed(src, &out.program);
        assert!(!checked.diagnostics.is_empty(), "expected a type error");
        assert!(
            checked.typed_hir.is_none(),
            "an unclean check must not produce typed HIR"
        );
    }

    #[test]
    fn mono_of_covers_the_scalar_and_boxed_split() {
        assert_eq!(mono_of(&Type::Prim(Prim::Int)), MonoTy::I64);
        assert_eq!(mono_of(&Type::Prim(Prim::Float)), MonoTy::F64);
        assert_eq!(mono_of(&Type::Prim(Prim::Bool)), MonoTy::Bool);
        assert_eq!(mono_of(&Type::Prim(Prim::Unit)), MonoTy::Unit);
        assert_eq!(mono_of(&Type::Bytes), MonoTy::BytesHandle);
        assert_eq!(mono_of(&Type::ByteBuffer), MonoTy::ByteBufferHandle);
        assert_eq!(mono_of(&Type::Prim(Prim::String)), MonoTy::Boxed);
        assert_eq!(mono_of(&Type::Unknown), MonoTy::Boxed);
        assert_eq!(mono_of(&Type::Var(0)), MonoTy::Boxed);
        assert_eq!(
            mono_of(&Type::Skolem {
                name: "T".into(),
                id: 1,
                origin: "test:T".into(),
            }),
            MonoTy::Boxed
        );
        assert_eq!(
            mono_of(&Type::Foreign {
                name: "ns.X".into(),
                args: vec![]
            }),
            MonoTy::Boxed
        );
        assert_eq!(
            mono_of(&Type::Enum {
                base: "Color".into(),
                args: vec![]
            }),
            MonoTy::Boxed
        );
    }

    #[test]
    fn byte_handles_and_direct_record_projection_are_exact_facts() {
        let src = "record Image { pixels: ByteBuffer, frozen: Bytes, n: int }\n\
             function paint(image: Image, direct: ByteBuffer) -> int {\n\
               let mut pixels = image.pixels\n\
               let frozen = image.frozen\n\
               direct.length()\n\
             }\n";
        let hir = typed_current(src);
        assert_eq!(hir.local_mono("direct"), Some(MonoTy::ByteBufferHandle));
        assert_eq!(hir.local_mono("pixels"), Some(MonoTy::ByteBufferHandle));
        assert_eq!(hir.local_mono("frozen"), Some(MonoTy::BytesHandle));
        assert_eq!(hir.byte_record_params.len(), 1);
        assert_eq!(
            hir.byte_record_params[0].declaration_identity,
            "__entry__::Image"
        );
        assert_eq!(hir.byte_record_params[0].fields.len(), 2);
        assert_eq!(hir.byte_projections.len(), 2);
        assert_eq!(hir.byte_projections[0].field, "pixels");
        assert_eq!(hir.byte_projections[0].mono, MonoTy::ByteBufferHandle);
        assert_eq!(hir.byte_projections[1].field, "frozen");
        assert_eq!(hir.byte_projections[1].mono, MonoTy::BytesHandle);

        let parsed = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::V5_20,
            },
        );
        assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);
        let checked = check_program_typed_with_version(src, &parsed.program, LangVersion::V5_20);
        assert!(checked.diagnostics.is_empty(), "{:#?}", checked.diagnostics);
        assert_eq!(
            checked
                .typed_hir
                .expect("clean 5.20 program yields typed HIR")
                .byte_record_params[0]
                .declaration_identity,
            "__entry__::Image"
        );
    }

    #[test]
    fn aliases_generics_nested_and_non_byte_fields_do_not_create_projection_facts() {
        let hir = typed_current(
            "record Generic<T> { bytes: ByteBuffer, value: T }\n\
             record Image { pixels: ByteBuffer, n: int }\n\
             type Alias = Image\n\
             function generic(x: Generic<int>) -> int { let b = x.bytes; b.length() }\n\
             function aliased(x: Alias) -> int { let b = x.pixels; b.length() }\n\
             function nested(x: Image) -> int { if true { let b = x.pixels; b.length() } else { 0 } }\n\
             function scalar(x: Image) -> int { let n = x.n; n }\n",
        );
        assert_eq!(hir.byte_record_params.len(), 2);
        assert!(hir.byte_record_params.iter().all(|fact| fact.name == "x"));
        assert!(hir.byte_projections.is_empty());
    }

    #[test]
    fn typed_check_preserves_the_selected_older_version() {
        let src = "enum Pair { Both(int, int) }\nlet pair = Pair.Both(1, 2)\n";
        let parsed = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::V5_4,
            },
        );
        assert!(parsed.diagnostics.is_empty(), "{:#?}", parsed.diagnostics);

        let old = check_program_typed_with_version(src, &parsed.program, LangVersion::V5_3);
        assert!(!old.diagnostics.is_empty(), "v5.3 must reject v5.4 arity");
        assert!(old.typed_hir.is_none());

        let current = check_program_typed_with_version(src, &parsed.program, LangVersion::CURRENT);
        assert!(current.diagnostics.is_empty(), "{:#?}", current.diagnostics);
        assert!(current.typed_hir.is_some());
    }

    #[test]
    fn full_typed_facts_cover_declarations_patterns_calls_and_stable_rigids() {
        let hir = typed_current(
            "record Box<T> { value: T }\n\
             enum Maybe<T> { Present(T) }\n\
             newtype Id<T> = T\n\
             function identity<T>(value: T) -> T { value }\n\
             let choose = (value: int) => match value { case n => identity(n) }\n\
             let answer = choose(42)\n",
        );
        for kind in [
            TypedNodeKind::Expression,
            TypedNodeKind::Pattern,
            TypedNodeKind::Binding,
            TypedNodeKind::Declaration,
            TypedNodeKind::Type,
        ] {
            assert!(
                hir.nodes.iter().any(|node| node.kind == kind),
                "missing {kind:?}: {:#?}",
                hir.nodes
            );
        }
        assert_eq!(hir.calls.len(), 2, "{:#?}", hir.calls);
        assert!(
            hir.nodes.iter().all(|node| !node.ty.has_hole()),
            "{:#?}",
            hir.nodes
        );
        assert!(
            hir.calls
                .iter()
                .all(|call| !call.callee_type.has_hole() && !call.result_type.has_hole()),
            "{:#?}",
            hir.calls
        );
        let declaration = hir
            .nodes
            .iter()
            .find(|node| node.kind == TypedNodeKind::Declaration)
            .expect("generic declaration");
        let SemanticType::Function {
            parameters, result, ..
        } = &declaration.ty
        else {
            panic!("generic declaration is not a function: {declaration:#?}");
        };
        assert!(matches!(
            parameters.as_slice(),
            [SemanticType::Rigid { origin, .. }] if origin.starts_with("source:")
        ));
        assert!(matches!(
            result.as_ref(),
            SemanticType::Rigid { origin, .. } if origin.starts_with("source:")
        ));
        let mut nominal_type_origins = hir
            .nodes
            .iter()
            .filter(|node| node.kind == TypedNodeKind::Type)
            .filter_map(|node| match &node.ty {
                SemanticType::Rigid { origin, .. } => Some(origin.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        nominal_type_origins.sort();
        nominal_type_origins.dedup();
        assert_eq!(nominal_type_origins.len(), 3, "{:#?}", hir.nodes);
    }

    /// Parse a program (no type check) and run the `contains_concurrent` walk
    /// directly on its AST — so a pattern-hiding case can be tested even when it
    /// would not type-check clean (a clean check is required only for the typed
    /// HIR, not for the syntactic concurrent-presence fact).
    fn walk_has_concurrent(src: &str) -> bool {
        let out = parse_with_options(
            FileId(0),
            src,
            ParseOptions {
                language_version: LangVersion::V5_3,
            },
        );
        assert!(
            out.diagnostics.is_empty(),
            "parse failed: {:?}",
            out.diagnostics
        );
        items_contain_concurrent(&out.program.items)
    }

    #[test]
    fn contains_concurrent_is_false_for_a_plain_unit() {
        // A clean scalar unit (the native-eligible shape) has no `concurrent`, so
        // the fact is false and native may elide loop checkpoints.
        let hir = typed("let mut i = 0\nwhile i < 10 { i = i + 1 }\ni\n");
        assert!(
            !hir.contains_concurrent,
            "a unit with no `concurrent` must read false"
        );
    }

    #[test]
    fn contains_concurrent_is_true_when_directly_present() {
        // The straightforward case: a top-level `concurrent` expression (arms are
        // newline-separated, not comma-separated).
        let hir = typed("concurrent {\n  a: 1\n  b: 2\n}\n");
        assert!(hir.contains_concurrent);
    }

    #[test]
    fn contains_concurrent_descends_a_range_pattern_endpoint() {
        // A `concurrent` hidden inside a
        // range PATTERN's endpoint expression. The unit types clean, yet the AST
        // really contains `ExprKind::Concurrent` — the fact MUST be true, or
        // native would wrongly elide a checkpoint a concurrent arm needs.
        let src =
            "let x = 0\nmatch x {\n  case (concurrent { a: 1 }).a .. 3 => 10\n  case _ => 20\n}\n";
        let hir = typed(src);
        assert!(
            hir.contains_concurrent,
            "a `concurrent` hidden in a range-pattern endpoint must set the fact true"
        );
    }

    #[test]
    fn contains_concurrent_descends_expression_bearing_pattern_positions() {
        // Lock the exhaustive pattern descent over the positions that actually
        // parse an arbitrary EXPRESSION (a range pattern's endpoints — the only
        // pattern shape with embedded `Expr`s the grammar admits an arbitrary
        // primary in). Walked on the raw AST: the SYNTACTIC fact must see the
        // `concurrent` regardless of type-checking. (Literal patterns require a
        // literal token, not an arbitrary parenthesized expression, so a
        // `concurrent` cannot syntactically hide there — but `pattern_has_concurrent`
        // still descends `Literal` exhaustively in case the grammar ever widens.)
        // Range pattern, the LOW endpoint.
        assert!(walk_has_concurrent(
            "let x = 0\nmatch x {\n  case (concurrent { a: 1 }).a .. 3 => 1\n  case _ => 0\n}\n"
        ));
        // Range pattern, the HIGH endpoint.
        assert!(walk_has_concurrent(
            "let x = 0\nmatch x {\n  case 0 .. (concurrent { a: 1 }).a => 1\n  case _ => 0\n}\n"
        ));
        // Or-pattern alternative: a range-with-concurrent as one alternative.
        assert!(walk_has_concurrent(
            "let x = 0\nmatch x {\n  case 0 .. (concurrent { a: 1 }).a | 9 => 1\n  case _ => 0\n}\n"
        ));
        // A plain unit with no `concurrent` anywhere stays false.
        assert!(!walk_has_concurrent(
            "let x = 0\nmatch x {\n  case 1 .. 3 => 1\n  case _ => 0\n}\n"
        ));
    }
}
