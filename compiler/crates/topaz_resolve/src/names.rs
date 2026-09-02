//! Name resolution for the module surface (SPEC v5.2 §17, CDR-002
//! §5): the single module lexical namespace, namespace bindings and
//! their one-member lookups, Form-B visibility, export rules, and
//! public-resolvable exported signatures.
//!
//! Deliberately NOT here: a general defined-before-use /
//! unresolved-identifier rule. The v5.1 base never had one (it is a
//! checker-era question), and the module rules only constrain what
//! they name: namespace misuse, private selections, export shapes.
//! An unqualified identifier that resolves to nothing is left alone.

use std::collections::{BTreeMap, BTreeSet};

use topaz_diag::{Diagnostic, Label, Span};
use topaz_syntax::ast::*;

use crate::codes;
use crate::{
    NameResolutionFacts, ResolveOutput, ResolvedDeclarationFact, ResolvedDeclarationKind,
    ResolvedExportFact, ResolvedModule, ResolvedNamespace, ResolvedReferenceFact,
    ResolvedReferenceRole, ResolvedScopeFact, ResolvedScopeKind,
};

/// What a module-top-level name is bound to.
#[derive(Debug, PartialEq)]
enum Binding {
    /// Form-A import: a compile-time namespace for `target`.
    Namespace {
        target: String,
    },
    /// Form-B import: a read-only imported binding.
    Imported {
        target: String,
        target_name: String,
    },
    Function,
    TypeAlias {
        exported: bool,
    },
    NominalType {
        exported: bool,
    },
    Let {
        mutable: bool,
    },
    Const,
}

/// What a module exports under a name.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Export {
    Value,
    Type,
}

struct ModuleTable {
    file: topaz_diag::FileId,
    bindings: BTreeMap<String, (Binding, Span)>,
    exports: BTreeMap<String, Export>,
}

impl ModuleTable {
    fn has_private_immutable_runtime_let(&self, name: &str) -> bool {
        matches!(
            self.bindings.get(name),
            Some((Binding::Let { mutable: false }, _))
        ) && !self.exports.contains_key(name)
    }
}

/// Runs the name checks over a resolved unit, appending diagnostics.
pub(crate) fn check(out: &mut ResolveOutput) {
    let mut tables: BTreeMap<&str, ModuleTable> = BTreeMap::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut facts = NameResolutionFacts::default();

    for module in &out.modules {
        let src = out.map.file(module.file).src();
        tables.insert(
            module.identity.as_str(),
            build_table(module, src, &mut diagnostics),
        );
    }

    for module in &out.modules {
        let src = out.map.file(module.file).src();
        let table = &tables[module.identity.as_str()];
        facts.scopes.push(ResolvedScopeFact {
            file: module.file,
            ordinal: 0,
            parent_ordinal: None,
            kind: ResolvedScopeKind::Module,
            owner: module.program.span,
        });
        let mut bindings: Vec<_> = table.bindings.iter().collect();
        bindings.sort_by_key(|(name, (_, span))| (span.lo, span.hi, name.as_str()));
        for (name, (binding, span)) in bindings {
            let (namespace, kind, target_module, target_name) = match binding {
                Binding::Namespace { target } => (
                    ResolvedNamespace::Module,
                    ResolvedDeclarationKind::NamespaceImport,
                    Some(target.clone()),
                    None,
                ),
                Binding::Imported {
                    target,
                    target_name,
                } => (
                    if matches!(
                        tables[target.as_str()].exports.get(target_name),
                        Some(Export::Type)
                    ) {
                        ResolvedNamespace::Type
                    } else {
                        ResolvedNamespace::Value
                    },
                    ResolvedDeclarationKind::SelectedImport,
                    Some(target.clone()),
                    Some(target_name.clone()),
                ),
                Binding::Function => (
                    ResolvedNamespace::Value,
                    ResolvedDeclarationKind::Function,
                    None,
                    None,
                ),
                Binding::TypeAlias { .. } => (
                    ResolvedNamespace::Type,
                    ResolvedDeclarationKind::TypeAlias,
                    None,
                    None,
                ),
                Binding::NominalType { .. } => (
                    ResolvedNamespace::Type,
                    ResolvedDeclarationKind::NominalType,
                    None,
                    None,
                ),
                Binding::Let { .. } => (
                    ResolvedNamespace::Value,
                    ResolvedDeclarationKind::Let,
                    None,
                    None,
                ),
                Binding::Const => (
                    ResolvedNamespace::Value,
                    ResolvedDeclarationKind::Const,
                    None,
                    None,
                ),
            };
            facts.declarations.push(ResolvedDeclarationFact {
                file: module.file,
                scope_ordinal: 0,
                name: name.clone(),
                namespace,
                kind,
                span: *span,
                exported: table.exports.contains_key(name),
                target_module,
                target_name,
            });
        }
        for (name, export) in &table.exports {
            let (_, span) = &table.bindings[name];
            facts.exports.push(ResolvedExportFact {
                file: module.file,
                name: name.clone(),
                namespace: if matches!(export, Export::Type) {
                    ResolvedNamespace::Type
                } else {
                    ResolvedNamespace::Value
                },
                declaration_span: *span,
            });
        }
        for item in &module.program.items {
            let item = match &item.kind {
                StmtKind::Export(inner) => inner.as_ref(),
                _ => item,
            };
            if let StmtKind::Protocol(declaration) = &item.kind {
                facts.declarations.push(ResolvedDeclarationFact {
                    file: module.file,
                    scope_ordinal: 0,
                    name: text(src, declaration.name.span).to_string(),
                    namespace: ResolvedNamespace::Type,
                    kind: ResolvedDeclarationKind::Protocol,
                    span: declaration.name.span,
                    exported: false,
                    target_module: None,
                    target_name: None,
                });
            }
        }
        check_imports(module, src, &tables, &mut diagnostics);
        check_export_signatures(module, src, table, &mut diagnostics);
        let mut walker = Walker {
            file: module.file,
            module: &module.identity,
            src,
            table,
            tables: &tables,
            diagnostics: &mut diagnostics,
            facts: &mut facts,
            locals: Vec::new(),
            next_scope_ordinal: 1,
            record_default_depth: 0,
        };
        walker.program(&module.program);
    }

    out.name_facts = facts;
    out.diagnostics.append(&mut diagnostics);
}

fn text(src: &str, span: Span) -> &str {
    &src[span.lo as usize..span.hi as usize]
}

/// Builds the single module lexical namespace (SPEC v5.2 §17):
/// namespace imports, selected imports, declarations, and bindings
/// collide as static errors.
fn build_table(
    module: &ResolvedModule,
    src: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> ModuleTable {
    let mut bindings: BTreeMap<String, (Binding, Span)> = BTreeMap::new();
    let mut exports: BTreeMap<String, Export> = BTreeMap::new();
    let mut insert = |name: String, binding: Binding, span: Span, diags: &mut Vec<Diagnostic>| {
        if let Some((_, first)) = bindings.get(&name) {
            let first = *first;
            diags.push(Diagnostic::error(
                codes::NAME_COLLISION,
                format!(
                    "`{name}` is already bound at this module's top level (first binding at byte {})",
                    first.lo
                ),
                Label::new(span, ""),
            ));
        } else {
            bindings.insert(name, (binding, span));
        }
    };

    for item in &module.program.items {
        collect_item(item, src, &mut insert, &mut exports, diagnostics, false);
    }
    ModuleTable {
        file: module.file,
        bindings,
        exports,
    }
}

fn collect_item(
    item: &Stmt,
    src: &str,
    insert: &mut impl FnMut(String, Binding, Span, &mut Vec<Diagnostic>),
    exports: &mut BTreeMap<String, Export>,
    diagnostics: &mut Vec<Diagnostic>,
    exported: bool,
) {
    match &item.kind {
        StmtKind::Import(import) => {
            let segments = &import.path.segments;
            let target: Vec<&str> = segments.iter().map(|s| text(src, s.span)).collect();
            let target = target.join(".");
            match &import.kind {
                ImportKind::Namespace { alias } => {
                    let bound = alias.unwrap_or(*segments.last().expect("nonempty path"));
                    insert(
                        text(src, bound.span).to_string(),
                        Binding::Namespace { target },
                        bound.span,
                        diagnostics,
                    );
                }
                ImportKind::Selected { specs } => {
                    for spec in specs {
                        let bound = spec.alias.unwrap_or(spec.name);
                        insert(
                            text(src, bound.span).to_string(),
                            Binding::Imported {
                                target: target.clone(),
                                target_name: text(src, spec.name.span).to_string(),
                            },
                            bound.span,
                            diagnostics,
                        );
                    }
                }
            }
        }
        StmtKind::Export(inner) => {
            collect_item(inner, src, insert, exports, diagnostics, true);
        }
        StmtKind::Function(decl) => {
            let name = text(src, decl.name.span).to_string();
            if exported {
                exports.insert(name.clone(), Export::Value);
            }
            insert(name, Binding::Function, decl.name.span, diagnostics);
        }
        StmtKind::TypeAlias(alias) => {
            let name = text(src, alias.name.span).to_string();
            if exported {
                exports.insert(name.clone(), Export::Type);
            }
            insert(
                name,
                Binding::TypeAlias { exported },
                alias.name.span,
                diagnostics,
            );
        }
        StmtKind::Enum(decl) => {
            let name = text(src, decl.name.span).to_string();
            if exported {
                exports.insert(name.clone(), Export::Type);
            }
            insert(
                name,
                Binding::NominalType { exported },
                decl.name.span,
                diagnostics,
            );
        }
        StmtKind::Record(decl) => {
            let name = text(src, decl.name.span).to_string();
            if exported {
                exports.insert(name.clone(), Export::Type);
            }
            insert(
                name,
                Binding::NominalType { exported },
                decl.name.span,
                diagnostics,
            );
        }
        StmtKind::Newtype(decl) => {
            let name = text(src, decl.name.span).to_string();
            if exported {
                exports.insert(name.clone(), Export::Type);
            }
            insert(
                name,
                Binding::NominalType { exported },
                decl.name.span,
                diagnostics,
            );
        }
        StmtKind::Let {
            mutable, pattern, ..
        } => {
            for ident in pattern_bindings(pattern, src) {
                let name = text(src, ident.span).to_string();
                if exported {
                    if *mutable {
                        diagnostics.push(Diagnostic::error(
                            codes::EXPORT_LET_MUT,
                            "`export let mut` is a static error: exported bindings are immutable views",
                            Label::new(ident.span, ""),
                        ));
                    } else {
                        exports.insert(name.clone(), Export::Value);
                    }
                }
                insert(
                    name,
                    Binding::Let { mutable: *mutable },
                    ident.span,
                    diagnostics,
                );
            }
        }
        StmtKind::Const { name, .. } => {
            let bound = text(src, name.span).to_string();
            if exported {
                exports.insert(bound.clone(), Export::Value);
            }
            insert(bound, Binding::Const, name.span, diagnostics);
        }
        _ => {}
    }
}

/// All module names introduced by a pattern. Same-bindings or-pattern
/// alternatives introduce one module binding per agreed name, not one binding
/// per alternative. A repeated name inside ONE alternative remains duplicated
/// here so the ordinary module collision diagnostic is preserved.
fn pattern_bindings(pattern: &Pattern, src: &str) -> Vec<Ident> {
    let mut out = Vec::new();
    collect_pattern(pattern, src, &mut out);
    out
}

fn collect_pattern(pattern: &Pattern, src: &str, out: &mut Vec<Ident>) {
    match &pattern.kind {
        PatternKind::Wildcard | PatternKind::Literal(_) | PatternKind::Range { .. } => {}
        PatternKind::Binding(name) => out.push(*name),
        PatternKind::Typed { name, .. } => out.push(*name),
        PatternKind::Or(alts) => {
            let mut seen_across = BTreeSet::new();
            for alt in alts {
                let mut alt_bindings = Vec::new();
                collect_pattern(alt, src, &mut alt_bindings);
                let mut seen_within = BTreeSet::new();
                for binding in alt_bindings {
                    let name = text(src, binding.span).to_string();
                    if !seen_within.insert(name.clone()) || seen_across.insert(name) {
                        out.push(binding);
                    }
                }
            }
        }
        PatternKind::Constructor { args, .. } => {
            for arg in args {
                collect_pattern(arg, src, out);
            }
        }
        PatternKind::List(elems) => {
            for elem in elems {
                match elem {
                    ListPatternElem::Pattern(p) => collect_pattern(p, src, out),
                    ListPatternElem::Rest(Some(p)) => collect_pattern(p, src, out),
                    ListPatternElem::Rest(None) => {}
                }
            }
        }
        PatternKind::Record(fields) | PatternKind::NominalRecord { fields, .. } => {
            for field in fields {
                match &field.pattern {
                    Some(p) => collect_pattern(p, src, out),
                    None => out.push(field.name),
                }
            }
        }
    }
}

/// Form-B visibility and zero-export checks (SPEC v5.2 §17).
fn check_imports(
    module: &ResolvedModule,
    src: &str,
    tables: &BTreeMap<&str, ModuleTable>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for item in &module.program.items {
        let StmtKind::Import(import) = &item.kind else {
            continue;
        };
        let segments: Vec<&str> = import
            .path
            .segments
            .iter()
            .map(|s| text(src, s.span))
            .collect();
        let target = segments.join(".");
        let target_table = &tables[target.as_str()];
        if target_table.exports.is_empty() {
            diagnostics.push(Diagnostic::error(
                codes::ZERO_EXPORT_IMPORT,
                format!("`{target}` exports nothing; v5.2 has no side-effect-only imports"),
                Label::new(import.path.span, ""),
            ));
            continue;
        }
        if let ImportKind::Selected { specs } = &import.kind {
            for spec in specs {
                let name = text(src, spec.name.span);
                if !target_table.exports.contains_key(name) {
                    let hint = topaz_diag::suggest::did_you_mean(
                        name,
                        target_table.exports.keys().map(String::as_str),
                    );
                    diagnostics.push(Diagnostic::error(
                        codes::NOT_EXPORTED,
                        format!("`{name}` is not exported by `{target}`{hint}"),
                        Label::new(spec.name.span, ""),
                    ));
                }
            }
        }
    }
}

/// Public-resolvable exported signatures (SPEC v5.2 §17): a named
/// type in an exported item's public surface must not be a
/// module-private type alias.
fn check_export_signatures(
    module: &ResolvedModule,
    src: &str,
    table: &ModuleTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for item in &module.program.items {
        let StmtKind::Export(inner) = &item.kind else {
            continue;
        };
        let mut check_type = |ty: &Type| {
            named_types(ty, &mut |name: Ident| {
                let name_text = text(src, name.span);
                if let Some((
                    Binding::TypeAlias { exported: false }
                    | Binding::NominalType { exported: false },
                    _,
                )) = table.bindings.get(name_text)
                {
                    diagnostics.push(Diagnostic::error(
                        codes::PRIVATE_TYPE_IN_EXPORT,
                        format!(
                            "`{name_text}` is a module-private type and may not appear in an exported public surface; export the type or use an inline structural type"
                        ),
                        Label::new(name.span, ""),
                    ));
                }
            });
        };
        match &inner.kind {
            StmtKind::Function(decl) => {
                for param in &decl.params {
                    check_type(&param.ty);
                }
                if let Some(ret) = &decl.return_type {
                    check_type(ret);
                }
            }
            StmtKind::TypeAlias(alias) => check_type(&alias.ty),
            StmtKind::Enum(decl) => {
                for variant in &decl.variants {
                    if let Some(payloads) = &variant.payload {
                        for ty in payloads {
                            check_type(ty);
                        }
                    }
                }
            }
            StmtKind::Record(decl) => {
                for field in &decl.fields {
                    check_type(&field.ty);
                }
            }
            StmtKind::Newtype(decl) => check_type(&decl.base),
            StmtKind::Let { ty: Some(ty), .. } => check_type(ty),
            _ => {}
        }
    }
}

fn named_types(ty: &Type, visit: &mut impl FnMut(Ident)) {
    match &ty.kind {
        TypeKind::Named { name, args } => {
            visit(*name);
            for arg in args {
                named_types(arg, visit);
            }
        }
        TypeKind::Qualified { args, .. } => {
            for arg in args {
                named_types(arg, visit);
            }
        }
        TypeKind::Record(fields) => {
            for field in fields {
                named_types(&field.ty, visit);
            }
        }
        TypeKind::Function { params, ret } => {
            for param in params {
                named_types(&param.ty, visit);
            }
            named_types(ret, visit);
        }
        TypeKind::Union(members) => {
            for member in members {
                named_types(member, visit);
            }
        }
        _ => {}
    }
}

/// Expression/type walker: namespace usage, one-member lookups,
/// read-only imports, qualified types. Locals shadow module names by
/// ordinary lexical scoping.
struct LocalScope<'a> {
    ordinal: u32,
    value_bindings: BTreeMap<&'a str, Span>,
    type_bindings: BTreeMap<&'a str, Span>,
}

struct ReferenceTarget {
    module: String,
    name: Option<String>,
    namespace: ResolvedNamespace,
    file: topaz_diag::FileId,
    span: Span,
}

enum ValueReferenceBinding {
    Imported,
    Namespace,
}

struct Walker<'a> {
    file: topaz_diag::FileId,
    module: &'a str,
    src: &'a str,
    table: &'a ModuleTable,
    tables: &'a BTreeMap<&'a str, ModuleTable>,
    diagnostics: &'a mut Vec<Diagnostic>,
    facts: &'a mut NameResolutionFacts,
    locals: Vec<LocalScope<'a>>,
    next_scope_ordinal: u32,
    record_default_depth: usize,
}

impl<'a> Walker<'a> {
    fn program(&mut self, program: &Program) {
        for item in &program.items {
            self.stmt(item);
        }
    }

    fn current_scope(&self) -> u32 {
        self.locals.last().map_or(0, |scope| scope.ordinal)
    }

    fn push_scope(
        &mut self,
        kind: ResolvedScopeKind,
        owner: Span,
        declarations: impl IntoIterator<Item = (&'a str, Span, ResolvedDeclarationKind)>,
    ) {
        let ordinal = self.next_scope_ordinal;
        self.next_scope_ordinal += 1;
        let parent_ordinal = Some(self.current_scope());
        let mut value_bindings = BTreeMap::new();
        for (name, span, declaration_kind) in declarations {
            value_bindings.insert(name, span);
            self.facts.declarations.push(ResolvedDeclarationFact {
                file: self.file,
                scope_ordinal: ordinal,
                name: name.to_string(),
                namespace: ResolvedNamespace::Value,
                kind: declaration_kind,
                span,
                exported: false,
                target_module: None,
                target_name: None,
            });
        }
        self.facts.scopes.push(ResolvedScopeFact {
            file: self.file,
            ordinal,
            parent_ordinal,
            kind,
            owner,
        });
        self.locals.push(LocalScope {
            ordinal,
            value_bindings,
            type_bindings: BTreeMap::new(),
        });
    }

    fn declare_local(
        &mut self,
        name: &'a str,
        span: Span,
        namespace: ResolvedNamespace,
        kind: ResolvedDeclarationKind,
    ) {
        let scope_ordinal = self.current_scope();
        let scope_index = self.locals.len() - 1;
        let scope = &mut self.locals[scope_index];
        match namespace {
            ResolvedNamespace::Value => {
                scope.value_bindings.insert(name, span);
            }
            ResolvedNamespace::Type => {
                scope.type_bindings.insert(name, span);
            }
            ResolvedNamespace::Module => unreachable!("local module bindings are imports"),
        }
        self.facts.declarations.push(ResolvedDeclarationFact {
            file: self.file,
            scope_ordinal,
            name: name.to_string(),
            namespace,
            kind,
            span,
            exported: false,
            target_module: None,
            target_name: None,
        });
    }

    fn local_target(&self, name: &str, namespace: ResolvedNamespace) -> Option<(u32, Span)> {
        self.locals.iter().rev().find_map(|scope| {
            let bindings = match namespace {
                ResolvedNamespace::Value => &scope.value_bindings,
                ResolvedNamespace::Type => &scope.type_bindings,
                ResolvedNamespace::Module => return None,
            };
            bindings
                .get(name)
                .copied()
                .map(|span| (scope.ordinal, span))
        })
    }

    fn record_reference(
        &mut self,
        name: String,
        namespace: ResolvedNamespace,
        role: ResolvedReferenceRole,
        span: Span,
        target: Option<ReferenceTarget>,
    ) {
        let (target_module, target_name, target_namespace, target_file, target_span) = target
            .map_or((None, None, None, None, None), |target| {
                (
                    Some(target.module),
                    target.name,
                    Some(target.namespace),
                    Some(target.file),
                    Some(target.span),
                )
            });
        self.facts.references.push(ResolvedReferenceFact {
            file: self.file,
            scope_ordinal: self.current_scope(),
            name,
            namespace,
            role,
            span,
            target_file,
            target_span,
            target_namespace,
            target_module,
            target_name,
        });
    }

    fn record_value_reference(
        &mut self,
        name: &str,
        span: Span,
        role: ResolvedReferenceRole,
    ) -> Option<ValueReferenceBinding> {
        if let Some((_, target_span)) = self.local_target(name, ResolvedNamespace::Value) {
            self.record_reference(
                name.to_string(),
                ResolvedNamespace::Value,
                role,
                span,
                Some(ReferenceTarget {
                    module: self.module.to_string(),
                    name: Some(name.to_string()),
                    namespace: ResolvedNamespace::Value,
                    file: self.file,
                    span: target_span,
                }),
            );
            return None;
        }
        match self.table.bindings.get(name) {
            Some((
                Binding::Imported {
                    target,
                    target_name,
                },
                local_span,
            )) => {
                let namespace = if matches!(
                    self.tables[target.as_str()].exports.get(target_name),
                    Some(Export::Type)
                ) {
                    ResolvedNamespace::Type
                } else {
                    ResolvedNamespace::Value
                };
                self.record_reference(
                    name.to_string(),
                    ResolvedNamespace::Value,
                    role,
                    span,
                    Some(ReferenceTarget {
                        module: target.clone(),
                        name: Some(target_name.clone()),
                        namespace,
                        file: self.file,
                        span: *local_span,
                    }),
                );
                Some(ValueReferenceBinding::Imported)
            }
            Some((Binding::Namespace { target }, target_span)) => {
                self.record_reference(
                    name.to_string(),
                    ResolvedNamespace::Module,
                    role,
                    span,
                    Some(ReferenceTarget {
                        module: target.clone(),
                        name: None,
                        namespace: ResolvedNamespace::Module,
                        file: self.file,
                        span: *target_span,
                    }),
                );
                Some(ValueReferenceBinding::Namespace)
            }
            Some((binding, target_span)) => {
                let namespace = if matches!(
                    binding,
                    Binding::TypeAlias { .. } | Binding::NominalType { .. }
                ) {
                    ResolvedNamespace::Type
                } else {
                    ResolvedNamespace::Value
                };
                self.record_reference(
                    name.to_string(),
                    ResolvedNamespace::Value,
                    role,
                    span,
                    Some(ReferenceTarget {
                        module: self.module.to_string(),
                        name: Some(name.to_string()),
                        namespace,
                        file: self.file,
                        span: *target_span,
                    }),
                );
                None
            }
            None => {
                self.record_reference(name.to_string(), ResolvedNamespace::Value, role, span, None);
                None
            }
        }
    }

    fn namespace_target(&self, name: &str) -> Option<&str> {
        if self.local_target(name, ResolvedNamespace::Value).is_some() {
            return None;
        }
        match self.table.bindings.get(name) {
            Some((Binding::Namespace { target }, _)) => Some(target),
            _ => None,
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Import(_) => {}
            StmtKind::Export(inner) => self.stmt(inner),
            StmtKind::Function(decl) => {
                if !self.locals.is_empty() {
                    self.declare_local(
                        text(self.src, decl.name.span),
                        decl.name.span,
                        ResolvedNamespace::Value,
                        ResolvedDeclarationKind::Function,
                    );
                }
                self.function_decl(decl);
            }
            StmtKind::TypeAlias(alias) => {
                if !self.locals.is_empty() {
                    self.declare_local(
                        text(self.src, alias.name.span),
                        alias.name.span,
                        ResolvedNamespace::Type,
                        ResolvedDeclarationKind::TypeAlias,
                    );
                }
                self.ty(&alias.ty);
            }
            // v5.3 enum: resolve type references in variant payloads (empty for a
            // payload-less variant, so this is a no-op for the MVP first slice).
            StmtKind::Enum(decl) => {
                if !self.locals.is_empty() {
                    self.declare_local(
                        text(self.src, decl.name.span),
                        decl.name.span,
                        ResolvedNamespace::Type,
                        ResolvedDeclarationKind::NominalType,
                    );
                }
                for v in &decl.variants {
                    if let Some(tys) = &v.payload {
                        for t in tys {
                            self.ty(t);
                        }
                    }
                }
            }
            // v5.4 record: resolve type references in each field's type, and any
            // default-value expression (a default may reference imported/global
            // names — never `self` or another field, which the checker enforces).
            StmtKind::Record(decl) => {
                if !self.locals.is_empty() {
                    self.declare_local(
                        text(self.src, decl.name.span),
                        decl.name.span,
                        ResolvedNamespace::Type,
                        ResolvedDeclarationKind::NominalType,
                    );
                }
                for f in &decl.fields {
                    self.ty(&f.ty);
                    if let Some(default) = &f.default {
                        self.record_default_depth += 1;
                        self.expr(default);
                        self.record_default_depth -= 1;
                    }
                }
            }
            // v5.4 newtype: resolve the type references in the base type
            // (`newtype UserId = int`, or `newtype Ids = Array<UserId>`).
            StmtKind::Newtype(decl) => {
                if !self.locals.is_empty() {
                    self.declare_local(
                        text(self.src, decl.name.span),
                        decl.name.span,
                        ResolvedNamespace::Type,
                        ResolvedDeclarationKind::NominalType,
                    );
                }
                self.ty(&decl.base);
            }
            // v5.4 impl: each method resolves like a free function — its params
            // (`self` first), return type, and body. The receiver type name itself
            // resolves like any nominal type reference. Method bodies see the
            // module's free functions + sibling methods (free-function dispatch).
            StmtKind::Impl(decl) => {
                for m in &decl.methods {
                    self.function_decl(&m.decl);
                }
            }
            // v5.4 §4 protocol: a protocol declares free-function method SIGNATURES.
            // Resolve each signature's parameter + return types (`Self`/`T` are type
            // stand-ins, not bindings — resolved by the checker's former, like any
            // nominal type reference). The bodies are empty placeholders (a protocol
            // method is unimplemented), so nothing to walk there.
            StmtKind::Protocol(decl) => {
                if !self.locals.is_empty() {
                    self.declare_local(
                        text(self.src, decl.name.span),
                        decl.name.span,
                        ResolvedNamespace::Type,
                        ResolvedDeclarationKind::Protocol,
                    );
                }
                for m in &decl.methods {
                    for param in &m.params {
                        self.ty(&param.ty);
                    }
                    if let Some(ret) = &m.return_type {
                        self.ty(ret);
                    }
                }
            }
            StmtKind::Let {
                pattern, ty, value, ..
            } => {
                if let Some(ty) = ty {
                    self.ty(ty);
                }
                self.pattern_types(pattern);
                self.expr(value);
                // Bindings join the innermost scope after their
                // initializer (block-local sequencing).
                if !self.locals.is_empty() {
                    for ident in pattern_bindings(pattern, self.src) {
                        self.declare_local(
                            text(self.src, ident.span),
                            ident.span,
                            ResolvedNamespace::Value,
                            ResolvedDeclarationKind::Let,
                        );
                    }
                }
            }
            StmtKind::Const { name, ty, value } => {
                if let Some(ty) = ty {
                    self.ty(ty);
                }
                self.expr(value);
                if !self.locals.is_empty() {
                    self.declare_local(
                        text(self.src, name.span),
                        name.span,
                        ResolvedNamespace::Value,
                        ResolvedDeclarationKind::Const,
                    );
                }
            }
            StmtKind::Using { name, value, body } => {
                self.expr(value);
                self.push_scope(
                    ResolvedScopeKind::Using,
                    body.span,
                    [(
                        text(self.src, name.span),
                        name.span,
                        ResolvedDeclarationKind::Using,
                    )],
                );
                self.block(body);
                self.locals.pop();
            }
            StmtKind::Assign { target, value, .. } => {
                // Form-A read-only rule (SPEC v5.2 §17): assignment
                // through a namespace member is rejected like any
                // imported-binding write.
                if let ExprKind::Member { object, field } = &target.kind
                    && let ExprKind::Ident = &object.kind
                {
                    let head = text(self.src, object.span);
                    if self.namespace_target(head).is_some() {
                        let member = text(self.src, field.span);
                        self.diagnostics.push(Diagnostic::error(
                            codes::READONLY_IMPORT,
                            format!(
                                "`{head}.{member}` is an exported binding viewed through a namespace and cannot be assigned"
                            ),
                            Label::new(target.span, ""),
                        ));
                        self.expr(value);
                        return;
                    }
                }
                if let ExprKind::Ident = &target.kind {
                    let name = text(self.src, target.span);
                    match self.record_value_reference(
                        name,
                        target.span,
                        ResolvedReferenceRole::Write,
                    ) {
                        Some(ValueReferenceBinding::Imported) => {
                            self.diagnostics.push(Diagnostic::error(
                                codes::READONLY_IMPORT,
                                format!("`{name}` is an imported binding and cannot be assigned"),
                                Label::new(target.span, ""),
                            ));
                        }
                        Some(ValueReferenceBinding::Namespace) => {
                            self.namespace_misuse(name, target.span);
                        }
                        _ => {}
                    }
                } else {
                    self.expr(target);
                }
                self.expr(value);
            }
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            StmtKind::Defer(body) => self.expr(body),
            StmtKind::Expr(body) => self.expr(body),
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            // A `break <value>` value references names; resolve it.
            // Labels are loop-local lexical markers, not value names — no binding.
            StmtKind::Break { value, .. } => {
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            StmtKind::Continue { .. } => {}
        }
    }

    fn function_decl(&mut self, decl: &FunctionDecl) {
        for param in &decl.params {
            self.ty(&param.ty);
            if let Some(default) = &param.default {
                self.expr(default);
            }
        }
        if let Some(ret) = &decl.return_type {
            self.ty(ret);
        }
        self.push_scope(
            ResolvedScopeKind::Function,
            decl.name.span.merge(decl.body.span),
            decl.params.iter().map(|param| {
                (
                    text(self.src, param.name.span),
                    param.name.span,
                    ResolvedDeclarationKind::Parameter,
                )
            }),
        );
        self.block(&decl.body);
        self.locals.pop();
    }

    fn block(&mut self, block: &Block) {
        self.push_scope(ResolvedScopeKind::Block, block.span, []);
        for stmt in &block.stmts {
            self.stmt(stmt);
        }
        if let Some(tail) = &block.tail {
            self.expr(tail);
        }
        self.locals.pop();
    }

    /// Walks the types carried by typed patterns (`let x: T = ...`
    /// routes its annotation through the pattern).
    fn pattern_types(&mut self, pattern: &Pattern) {
        match &pattern.kind {
            PatternKind::Typed { ty, .. } => self.ty(ty),
            PatternKind::Or(alts) => {
                for alt in alts {
                    self.pattern_types(alt);
                }
            }
            PatternKind::Constructor { args, .. } => {
                for arg in args {
                    self.pattern_types(arg);
                }
            }
            PatternKind::List(elems) => {
                for elem in elems {
                    match elem {
                        ListPatternElem::Pattern(p) => self.pattern_types(p),
                        ListPatternElem::Rest(Some(p)) => self.pattern_types(p),
                        ListPatternElem::Rest(None) => {}
                    }
                }
            }
            PatternKind::Record(fields) | PatternKind::NominalRecord { fields, .. } => {
                for field in fields {
                    if let Some(p) = &field.pattern {
                        self.pattern_types(p);
                    }
                }
            }
            _ => {}
        }
    }

    fn namespace_misuse(&mut self, name: &str, span: Span) {
        self.diagnostics.push(Diagnostic::error(
            codes::NAMESPACE_NOT_VALUE,
            format!("`{name}` is a namespace, not a value; it may appear only as `{name}.member`"),
            Label::new(span, ""),
        ));
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Ident => {
                let name = text(self.src, expr.span);
                if matches!(
                    self.record_value_reference(name, expr.span, ResolvedReferenceRole::Read),
                    Some(ValueReferenceBinding::Namespace)
                ) {
                    self.namespace_misuse(name, expr.span);
                }
            }
            ExprKind::Member { object, field } => {
                // Namespace member lookup consumes exactly one
                // exported Identifier member (SPEC v5.2 §17).
                if let ExprKind::Ident = &object.kind {
                    let head = text(self.src, object.span);
                    if let Some(target) = self.namespace_target(head).map(str::to_string) {
                        let member = text(self.src, field.span);
                        let target_table = &self.tables[target.as_str()];
                        let exports = &target_table.exports;
                        let export = exports.get(member);
                        let target_span = target_table.bindings.get(member).map(|(_, span)| *span);
                        self.record_reference(
                            format!("{head}.{member}"),
                            ResolvedNamespace::Value,
                            ResolvedReferenceRole::NamespaceMember,
                            field.span,
                            target_span.map(|span| ReferenceTarget {
                                module: target.clone(),
                                name: Some(member.to_string()),
                                namespace: if matches!(export, Some(Export::Type)) {
                                    ResolvedNamespace::Type
                                } else {
                                    ResolvedNamespace::Value
                                },
                                file: target_table.file,
                                span,
                            }),
                        );
                        if topaz_syntax::Keyword::lookup(member).is_some() {
                            self.diagnostics.push(Diagnostic::error(
                                codes::NAMESPACE_MEMBER_KIND,
                                format!(
                                    "namespace members are exported declarations and cannot be keyword-named (`{head}.{member}`)"
                                ),
                                Label::new(field.span, ""),
                            ));
                        } else {
                            match export {
                                Some(Export::Type) => {
                                    self.diagnostics.push(Diagnostic::error(
                                        codes::NAMESPACE_MEMBER_KIND,
                                        format!(
                                            "`{member}` is an exported type alias of `{target}`, not a value; use it in type position"
                                        ),
                                        Label::new(field.span, ""),
                                    ));
                                }
                                Some(_) => {}
                                None => {
                                    if self.record_default_depth > 0
                                        && target_table.has_private_immutable_runtime_let(member)
                                    {
                                        return;
                                    }
                                    // Value position: only non-type exports are
                                    // usable here, so a type alias is not offered.
                                    let hint = topaz_diag::suggest::did_you_mean(
                                        member,
                                        exports
                                            .iter()
                                            .filter(|(_, e)| !matches!(e, Export::Type))
                                            .map(|(k, _)| k.as_str()),
                                    );
                                    self.diagnostics.push(Diagnostic::error(
                                        codes::NOT_EXPORTED,
                                        format!("`{member}` is not exported by `{target}`{hint}"),
                                        Label::new(field.span, ""),
                                    ));
                                }
                            }
                        }
                        return;
                    }
                }
                self.expr(object);
            }
            ExprKind::Paren(inner) | ExprKind::Try(inner) => self.expr(inner),
            ExprKind::Block(block) => self.block(block),
            ExprKind::If {
                cond,
                then_block,
                else_branch,
            } => {
                self.expr(cond);
                self.block(then_block);
                if let Some(else_branch) = else_branch {
                    self.expr(else_branch);
                }
            }
            ExprKind::Match { scrutinee, cases } => {
                self.expr(scrutinee);
                for case in cases {
                    self.pattern_types(&case.pattern);
                    self.push_scope(
                        ResolvedScopeKind::Pattern,
                        case.pattern.span,
                        pattern_bindings(&case.pattern, self.src)
                            .into_iter()
                            .map(|ident| {
                                (
                                    text(self.src, ident.span),
                                    ident.span,
                                    ResolvedDeclarationKind::Pattern,
                                )
                            }),
                    );
                    if let Some(guard) = &case.guard {
                        self.expr(guard);
                    }
                    match &case.body {
                        CaseArmBody::Expr(body) => self.expr(body),
                        CaseArmBody::Return { value: Some(v), .. } => self.expr(v),
                        CaseArmBody::Return { value: None, .. } => {}
                    }
                    self.locals.pop();
                }
            }
            ExprKind::For {
                pattern,
                iter,
                body,
            } => {
                self.expr(iter);
                self.pattern_types(pattern);
                self.push_scope(
                    ResolvedScopeKind::Pattern,
                    pattern.span,
                    pattern_bindings(pattern, self.src)
                        .into_iter()
                        .map(|ident| {
                            (
                                text(self.src, ident.span),
                                ident.span,
                                ResolvedDeclarationKind::Pattern,
                            )
                        }),
                );
                self.block(body);
                self.locals.pop();
            }
            // A `loop` body is a fresh inner scope (like `while`/`for`);
            // the optional label binds no value name.
            ExprKind::Loop { body, .. } => {
                self.block(body);
            }
            ExprKind::Concurrent {
                timeout,
                arms,
                else_block,
            } => {
                if let Some(timeout) = timeout {
                    self.expr(timeout);
                }
                for arm in arms {
                    self.expr(&arm.value);
                }
                if let Some(else_block) = else_block {
                    self.block(else_block);
                }
            }
            ExprKind::Call { callee, args, .. } => {
                self.expr(callee);
                for arg in args {
                    match arg {
                        CallArg::Positional(e) | CallArg::Spread(e) => self.expr(e),
                        CallArg::Named { value, .. } => self.expr(value),
                    }
                }
            }
            ExprKind::Index { object, index } => {
                self.expr(object);
                self.expr(index);
            }
            ExprKind::OptionalAccess { object, .. } => self.expr(object),
            ExprKind::Unary { operand, .. } => self.expr(operand),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Range { lo, hi, step, .. } => {
                self.expr(lo);
                self.expr(hi);
                if let Some(step) = step {
                    self.expr(step);
                }
            }
            ExprKind::Compose { lhs, rhs } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Pipe { lhs, rhs } => {
                self.expr(lhs);
                match rhs.as_ref() {
                    PipeRhs::Expr(e) => self.expr(e),
                    PipeRhs::Field(_) => {}
                }
            }
            ExprKind::Lambda { params, body } => {
                for param in params {
                    if let Some(ty) = &param.ty {
                        self.ty(ty);
                    }
                }
                self.push_scope(
                    ResolvedScopeKind::Lambda,
                    expr.span,
                    params.iter().map(|param| {
                        (
                            text(self.src, param.name.span),
                            param.name.span,
                            ResolvedDeclarationKind::Parameter,
                        )
                    }),
                );
                self.expr(body);
                self.locals.pop();
            }
            ExprKind::RecordLiteral { fields } => {
                for field in fields {
                    self.expr(&field.value);
                }
            }
            ExprKind::RecordUpdate {
                base,
                spread,
                fields,
            } => {
                self.expr(base);
                if let Some(spread) = spread {
                    self.expr(spread);
                }
                for field in fields {
                    self.expr(&field.value);
                }
            }
            ExprKind::Array(elements) => {
                for element in elements {
                    match element {
                        ArrayElement::Expr(e) | ArrayElement::Spread(e) => self.expr(e),
                    }
                }
            }
            ExprKind::SetLiteral(elements) => {
                for e in elements {
                    self.expr(e);
                }
            }
            ExprKind::MapLiteral(entries) => {
                for (k, v) in entries {
                    self.expr(k);
                    self.expr(v);
                }
            }
            // §6.4 comprehension: each `for`-clause pattern binds in a FRESH scope
            // visible to every later clause AND the body (left-to-right, nested like
            // loops); an `if`-clause resolves under the bindings in scope so far. The
            // accumulator lives in the engines (Rust-side), not as a Topaz binding, so
            // a body variable named `acc` is just an ordinary reference — nothing to
            // collide with (hygiene).
            ExprKind::Comprehension { clauses, body, .. } => {
                let mut pushed = 0usize;
                for clause in clauses {
                    match clause {
                        CompClause::For { pattern, iter } => {
                            self.expr(iter);
                            self.pattern_types(pattern);
                            self.push_scope(
                                ResolvedScopeKind::Comprehension,
                                pattern.span,
                                pattern_bindings(pattern, self.src)
                                    .into_iter()
                                    .map(|ident| {
                                        (
                                            text(self.src, ident.span),
                                            ident.span,
                                            ResolvedDeclarationKind::Pattern,
                                        )
                                    }),
                            );
                            pushed += 1;
                        }
                        CompClause::If(cond) => self.expr(cond),
                    }
                }
                match body.as_ref() {
                    CompBody::Elem(e) => self.expr(e),
                    CompBody::Entry { key, value } => {
                        self.expr(key);
                        self.expr(value);
                    }
                }
                for _ in 0..pushed {
                    self.locals.pop();
                }
            }
            ExprKind::String(lit) => {
                for part in &lit.parts {
                    if let StringPart::Interpolation(e) = part {
                        self.expr(e);
                    }
                }
            }
            _ => {}
        }
    }

    fn ty(&mut self, ty: &Type) {
        if let TypeKind::Named { name, .. } = &ty.kind {
            let value = text(self.src, name.span);
            let target_span = self
                .local_target(value, ResolvedNamespace::Type)
                .map(|(_, span)| span)
                .or_else(|| {
                    self.table.bindings.get(value).and_then(|(binding, span)| {
                        matches!(
                            binding,
                            Binding::TypeAlias { .. }
                                | Binding::NominalType { .. }
                                | Binding::Imported { .. }
                        )
                        .then_some(*span)
                    })
                });
            self.record_reference(
                value.to_string(),
                ResolvedNamespace::Type,
                ResolvedReferenceRole::Type,
                name.span,
                target_span.map(|span| ReferenceTarget {
                    module: self.module.to_string(),
                    name: Some(value.to_string()),
                    namespace: ResolvedNamespace::Type,
                    file: self.file,
                    span,
                }),
            );
        }
        if let TypeKind::Qualified { ns, name, args } = &ty.kind {
            let head = text(self.src, ns.span);
            let target = self.namespace_target(head).map(str::to_string);
            if target.is_none() {
                // ADR-081: a qualified named type is valid only when
                // name resolution proves the head is a namespace
                // binding — shadowed, non-namespace, and unbound
                // heads are all rejected.
                self.diagnostics.push(Diagnostic::error(
                    codes::NAMESPACE_MEMBER_KIND,
                    format!(
                        "the head of a qualified type must be a namespace binding; `{head}` is not one here"
                    ),
                    Label::new(ns.span, ""),
                ));
            }
            if let Some(target) = target {
                let member = text(self.src, name.span);
                let target_table = &self.tables[target.as_str()];
                let target_span = target_table.bindings.get(member).map(|(_, span)| *span);
                self.record_reference(
                    format!("{head}.{member}"),
                    ResolvedNamespace::Type,
                    ResolvedReferenceRole::Type,
                    name.span,
                    target_span.map(|span| ReferenceTarget {
                        module: target.clone(),
                        name: Some(member.to_string()),
                        namespace: ResolvedNamespace::Type,
                        file: target_table.file,
                        span,
                    }),
                );
                match target_table.exports.get(member) {
                    Some(Export::Type) => {}
                    Some(_) => self.diagnostics.push(Diagnostic::error(
                        codes::NAMESPACE_MEMBER_KIND,
                        format!("`{member}` is exported by `{target}` but is not a type alias"),
                        Label::new(name.span, ""),
                    )),
                    None => {
                        // Type position: only type-alias exports are usable here.
                        let hint = topaz_diag::suggest::did_you_mean(
                            member,
                            target_table
                                .exports
                                .iter()
                                .filter(|(_, e)| matches!(e, Export::Type))
                                .map(|(k, _)| k.as_str()),
                        );
                        self.diagnostics.push(Diagnostic::error(
                            codes::NOT_EXPORTED,
                            format!("`{member}` is not exported by `{target}`{hint}"),
                            Label::new(name.span, ""),
                        ))
                    }
                }
            }
            for arg in args {
                self.ty(arg);
            }
            return;
        }
        // Recurse for nested qualified types.
        match &ty.kind {
            TypeKind::Named { args, .. } => {
                for arg in args {
                    self.ty(arg);
                }
            }
            TypeKind::Record(fields) => {
                for field in fields {
                    self.ty(&field.ty);
                }
            }
            TypeKind::Function { params, ret } => {
                for param in params {
                    self.ty(&param.ty);
                }
                self.ty(ret);
            }
            TypeKind::Union(members) => {
                for member in members {
                    self.ty(member);
                }
            }
            _ => {}
        }
    }
}
