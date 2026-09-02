//! Module-aware unit checking (CDR-004 C-6, SPEC §17).
//!
//! Modules check in dependency order; each defining module's
//! exported `let`/`const`/`function`/`type` signatures become the
//! import surface of its importers. Namespace bindings resolve
//! member access and qualified types; selected imports bind values
//! and type aliases directly. Inside a unit the name space is
//! closed, so TPZ5002 (unbound name) graduates to compile time.

use std::collections::{BTreeMap, HashMap, HashSet};

use topaz_diag::{Code, Diagnostic, Label};
use topaz_syntax::LangVersion;
use topaz_syntax::ast;

use crate::CheckOutput;
use crate::expr::ExprChecker;
use crate::form::{Former, MethodInfo, ModuleContext, nominal_instance_id};
use crate::ty::{Ctor, Prim, Type};

/// One module of a compilation unit, in resolver discovery order.
pub struct UnitModule<'a> {
    /// Dotted logical identity (`utils.strings`).
    pub identity: String,
    /// True only for the CLI/package entry module. An exported `main` in this
    /// module is the v5.4 explicit entrypoint; exported `main` in dependencies is
    /// an ordinary function export.
    pub is_entry: bool,
    /// True for generated manifest extern modules. These modules publish a typed
    /// import surface but their bodies are not checked until the shared extern
    /// leaf lands.
    pub is_extern: bool,
    /// True only for a compiler-owned package-capability module. This bit is
    /// resolver provenance, not a property inferred from its path or source.
    pub is_generated_std: bool,
    /// Present when the package provider could not load/validate this extern
    /// module's deterministic replay fixture.
    pub extern_replay_error: Option<String>,
    pub src: &'a str,
    pub program: &'a ast::Program,
}

/// An exported value binding: its type plus the callable metadata a
/// consumer's call typing needs (rank-1 vars, required arity).
#[derive(Debug, Clone)]
pub struct ExportedValue {
    pub ty: Type,
    pub vars: u32,
    /// Protocol bounds aligned with the exported function's type variables.
    pub bounds: Vec<Vec<String>>,
    pub required: usize,
    /// Parameter names for named-argument call typing (§5).
    pub names: Vec<String>,
    /// Whether `names` is authoritative (declared functions) rather
    /// than absent (exported lambdas/function-typed values, whose
    /// named arguments stay unjudged at the consumer).
    pub names_known: bool,
    /// Per fixed parameter: declared with a default (§5 spread-skip
    /// checking); empty means the prefix rule.
    pub defaulted: Vec<bool>,
    /// Nominal metadata referenced by this exported value's written signature.
    /// This lets facades render `D.Msg` with dependency metadata even when the
    /// entry module also exports a bare `Msg`.
    pub nominals: ExportedNominals,
}

/// An exported type alias: the body resolved at the defining module
/// with `Var(i)` placeholders for its parameters.
#[derive(Debug, Clone)]
pub struct ExportedAlias {
    /// Logical module that owns the alias body and every unqualified nominal
    /// reference already resolved into it.
    pub defining_module: String,
    pub params: usize,
    pub body: Type,
    /// Nominal declarations reachable from the resolved body, including private
    /// declarations exposed through an exported alias.
    pub nominals: ExportedNominals,
}

#[derive(Debug, Clone)]
pub struct ExportedRecordField {
    pub name: String,
    pub ty: Type,
    pub has_default: bool,
}

#[derive(Debug, Clone)]
pub struct ExportedRecord {
    pub id: String,
    pub params: usize,
    pub fields: Vec<ExportedRecordField>,
    /// Nominal declarations reachable from field types. They remain checker
    /// metadata and do not bind their source names in the importing module.
    pub nominals: ExportedNominals,
}

#[derive(Debug, Clone)]
pub struct ExportedEnumVariant {
    pub name: String,
    pub payloads: Vec<Type>,
}

#[derive(Debug, Clone)]
pub struct ExportedEnum {
    pub id: String,
    pub params: usize,
    pub variants: Vec<ExportedEnumVariant>,
    /// Nominal declarations reachable from variant payload types.
    pub nominals: ExportedNominals,
}

#[derive(Debug, Clone)]
pub struct ExportedNewtype {
    pub id: String,
    pub params: usize,
    pub base: Type,
    /// Nominal declarations reachable from the wrapped base type.
    pub nominals: ExportedNominals,
}

#[derive(Debug, Clone, Default)]
pub struct ExportedNominals {
    pub records: HashMap<String, ExportedRecord>,
    pub enums: HashMap<String, ExportedEnum>,
    pub newtypes: HashMap<String, ExportedNewtype>,
}

/// One exported inherent receiver method. It accompanies its exported nominal;
/// it is never a standalone namespace value or selected-import name.
#[derive(Debug, Clone)]
pub struct ExportedReceiverMethod {
    /// Stable runtime identity `(defining module, nominal)` used by both emitters.
    pub dispatch_id: String,
    pub info: MethodInfo,
}

/// The export surface of one checked module.
#[derive(Debug, Clone, Default)]
pub struct ModuleExports {
    pub values: HashMap<String, ExportedValue>,
    pub private_runtime_values: HashMap<String, ExportedValue>,
    /// Namespace-private runtime values whose inferred type contained an
    /// unnameable projection in the defining module. Their stored type is
    /// gradualized so no module-local skolem crosses the boundary, while this
    /// taint keeps record-default lookup fail-closed instead of silently
    /// accepting the erased projection.
    pub private_runtime_projection_tainted: std::collections::HashSet<String>,
    pub aliases: HashMap<String, ExportedAlias>,
    /// Exported v5.4 nominal records. These are type-only exports: a record
    /// declaration does not create a runtime namespace field.
    pub records: HashMap<String, ExportedRecord>,
    /// Exported nominal enums. Type-only, but their variant payload metadata is
    /// part of the checked surface used by package importers and web facades.
    pub enums: HashMap<String, ExportedEnum>,
    /// Exported nominal newtypes. Type-only, carrying the base type for importers
    /// and ABI facade generation.
    pub newtypes: HashMap<String, ExportedNewtype>,
    /// Exported inherent receiver methods, keyed first by the exported nominal
    /// declaration name and then by method name.
    pub receiver_methods: HashMap<String, HashMap<String, ExportedReceiverMethod>>,
    /// Protocol conformances declared in this module. Importers thread these into
    /// their local checker so `Protocol.method(imported_nominal)` can see derived
    /// conformances from dependency modules.
    pub conformances: Vec<(String, String)>,
    /// True for a namespace whose exports are not known (an import
    /// cycle or a module outside the unit): member access on it
    /// stays silent instead of mis-reporting.
    pub ambient: bool,
    /// True for a manifest extern module surface.
    pub is_extern: bool,
    /// True only when resolver provenance marks the defining module as a
    /// compiler-owned package capability module.
    pub is_generated_std: bool,
    pub extern_replay_error: Option<String>,
}

/// Checks a whole unit in dependency order and returns every module's
/// diagnostics, at the CURRENT language version (the convenience entry).
pub fn check_unit(modules: &[UnitModule<'_>]) -> CheckOutput {
    check_unit_with_version(modules, LangVersion::CURRENT)
}

/// [`check_unit`] pinned to a language `version` — so v5.4-only enum features
/// gate by edition (the CLI threads the `--language-version` selection).
pub fn check_unit_with_version(modules: &[UnitModule<'_>], version: LangVersion) -> CheckOutput {
    check_modules_with_version(modules, version, false).output
}

/// Module-aware checker execution that additionally retains the semantic facts
/// needed to construct the complete Typed IR. Facts are gathered in dependency
/// order and published only when the whole unit is clean.
pub(crate) struct TypedModuleCheck {
    pub(crate) output: CheckOutput,
    pub(crate) locals: Vec<(String, topaz_diag::Span, Type)>,
    pub(crate) nodes: Vec<(topaz_hir::TypedNodeKind, topaz_diag::Span, Type)>,
    pub(crate) call_targets: Vec<(String, topaz_diag::Span, String)>,
    pub(crate) call_callees: Vec<(String, topaz_diag::Span, Type)>,
}

struct CompletedModuleCheck {
    diagnostics: Vec<Diagnostic>,
    surface: ModuleExports,
    conformances: Vec<(String, String)>,
    aliases: BTreeMap<String, ExportedAlias>,
    locals: Vec<(String, topaz_diag::Span, Type)>,
    nodes: Vec<(topaz_hir::TypedNodeKind, topaz_diag::Span, Type)>,
    call_targets: Vec<(topaz_diag::Span, String)>,
    call_callees: Vec<(topaz_diag::Span, Type)>,
}

/// Module-aware typed checking pinned to the caller-selected language version.
/// Typed-IR production remains pinned to the caller-selected language mode.
pub(crate) fn check_module_typed_with_version(
    modules: &[UnitModule<'_>],
    version: LangVersion,
) -> TypedModuleCheck {
    check_modules_with_version(modules, version, true)
}

/// One module-unit execution authority for diagnostic-only and typed callers.
/// The caller choice controls only typed fact retention; dependency order,
/// extern admission, entrypoint validation, export surfaces, aliases, and
/// conformances are identical in both products.
fn check_modules_with_version(
    modules: &[UnitModule<'_>],
    version: LangVersion,
    collect_typed: bool,
) -> TypedModuleCheck {
    let index: HashMap<&str, usize> = modules
        .iter()
        .enumerate()
        .map(|(i, m)| (m.identity.as_str(), i))
        .collect();
    let order = dependency_order(modules, &index);

    let mut exports: HashMap<String, ModuleExports> = HashMap::new();
    let mut local_aliases: BTreeMap<String, BTreeMap<String, ExportedAlias>> = BTreeMap::new();
    let mut diagnostics = Vec::new();
    let mut typed_locals: Vec<(String, topaz_diag::Span, Type)> = Vec::new();
    let mut typed_nodes = Vec::new();
    let mut typed_call_targets = Vec::new();
    let mut typed_call_callees = Vec::new();
    let mut conformances = Vec::new();
    for i in order {
        let m = &modules[i];
        let CompletedModuleCheck {
            diagnostics: mut diags,
            surface,
            conformances: module_conformances,
            aliases: module_aliases,
            locals,
            nodes,
            call_targets,
            call_callees,
        } = if m.is_extern {
            let (diags, surface, conformances) = extern_module_surface(m, version);
            CompletedModuleCheck {
                diagnostics: diags,
                surface,
                conformances,
                aliases: BTreeMap::new(),
                locals: Vec::new(),
                nodes: Vec::new(),
                call_targets: Vec::new(),
                call_callees: Vec::new(),
            }
        } else {
            check_module(m, &exports, collect_typed, version)
        };
        if m.is_entry {
            validate_explicit_main_signature(m, &surface, &mut diags);
        }
        diagnostics.extend(diags);
        typed_locals.extend(locals);
        typed_nodes.extend(nodes);
        typed_call_targets.extend(
            call_targets
                .into_iter()
                .map(|(span, target)| (m.identity.clone(), span, target)),
        );
        typed_call_callees.extend(
            call_callees
                .into_iter()
                .map(|(span, ty)| (m.identity.clone(), span, ty)),
        );
        conformances.extend(module_conformances);
        local_aliases.insert(m.identity.clone(), module_aliases);
        exports.insert(m.identity.clone(), surface);
    }
    conformances.sort();
    conformances.dedup();
    TypedModuleCheck {
        output: CheckOutput {
            diagnostics,
            exports: exports.into_iter().collect::<BTreeMap<_, _>>(),
            local_aliases,
            conformances,
        },
        locals: typed_locals,
        nodes: typed_nodes,
        call_targets: typed_call_targets,
        call_callees: typed_call_callees,
    }
}

/// Imported modules come before their importers; cycles (already
/// resolver-diagnosed) and unknown targets fall back to the given
/// order.
fn dependency_order(modules: &[UnitModule<'_>], index: &HashMap<&str, usize>) -> Vec<usize> {
    let mut order: Vec<usize> = Vec::with_capacity(modules.len());
    let mut state = vec![0u8; modules.len()]; // 0 new, 1 visiting, 2 done
    fn visit(
        i: usize,
        modules: &[UnitModule<'_>],
        index: &HashMap<&str, usize>,
        state: &mut [u8],
        order: &mut Vec<usize>,
    ) {
        if state[i] != 0 {
            return;
        }
        state[i] = 1;
        for target in import_targets(modules[i].src, modules[i].program) {
            if let Some(&j) = index.get(target.as_str())
                && state[j] == 0
            {
                visit(j, modules, index, state, order);
            }
        }
        state[i] = 2;
        order.push(i);
    }
    for i in 0..modules.len() {
        visit(i, modules, index, &mut state, &mut order);
    }
    order
}

fn import_targets(src: &str, program: &ast::Program) -> Vec<String> {
    let mut targets = Vec::new();
    for stmt in &program.items {
        if let ast::StmtKind::Import(item) = &stmt.kind {
            targets.push(dotted(src, item));
        }
    }
    targets
}

fn dotted(src: &str, item: &ast::ImportItem) -> String {
    item.path
        .segments
        .iter()
        .map(|s| &src[s.span.lo as usize..s.span.hi as usize])
        .collect::<Vec<_>>()
        .join(".")
}

fn text(src: &str, span: topaz_diag::Span) -> &str {
    &src[span.lo as usize..span.hi as usize]
}

/// Rebinds every nominal base through one identity map. The shared structural
/// transform owns the inventory of nested type positions; a replaced nominal
/// recursively remaps its generic arguments before taking ownership of the node.
fn remap_nominal_identities(ty: &Type, replacements: &HashMap<String, String>) -> Type {
    ty.transform_components(&mut |component| match component {
        Type::Enum { base, args } => replacements.get(base).map(|replacement| Type::Enum {
            base: replacement.clone(),
            args: args
                .iter()
                .map(|argument| remap_nominal_identities(argument, replacements))
                .collect(),
        }),
        Type::NominalRecord { base, args } => {
            replacements
                .get(base)
                .map(|replacement| Type::NominalRecord {
                    base: replacement.clone(),
                    args: args
                        .iter()
                        .map(|argument| remap_nominal_identities(argument, replacements))
                        .collect(),
                })
        }
        Type::Newtype { base, args } => replacements.get(base).map(|replacement| Type::Newtype {
            base: replacement.clone(),
            args: args
                .iter()
                .map(|argument| remap_nominal_identities(argument, replacements))
                .collect(),
        }),
        _ => None,
    })
}

fn extend_imported_conformances(
    target: &mut HashSet<(String, String)>,
    source: &[(String, String)],
    mut local_type_id: impl FnMut(&str) -> String,
) {
    target.extend(
        source
            .iter()
            .map(|(protocol, type_id)| (protocol.clone(), local_type_id(type_id))),
    );
}

fn function_type_params<'a>(src: &'a str, decl: &'a ast::FunctionDecl) -> HashMap<&'a str, Type> {
    decl.type_params
        .iter()
        .enumerate()
        .map(|(i, p)| (text(src, p.span), Type::Var(i as u32)))
        .collect()
}

fn function_bound_names(src: &str, decl: &ast::FunctionDecl) -> Vec<Vec<String>> {
    (0..decl.type_params.len())
        .map(|i| {
            decl.type_param_bounds
                .get(i)
                .map(|bounds| {
                    bounds
                        .iter()
                        .map(|bound| text(src, bound.span).to_string())
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect()
}

fn extern_module_surface(
    m: &UnitModule<'_>,
    version: LangVersion,
) -> (Vec<Diagnostic>, ModuleExports, Vec<(String, String)>) {
    let mut former = Former::with_version(m.src, m.program, version);
    former.validate_aliases();
    let mut surface = ModuleExports {
        is_extern: true,
        extern_replay_error: m.extern_replay_error.clone(),
        ..ModuleExports::default()
    };
    for stmt in &m.program.items {
        let ast::StmtKind::Export(inner) = &stmt.kind else {
            continue;
        };
        let ast::StmtKind::Function(decl) = &inner.kind else {
            continue;
        };
        let name = text(m.src, decl.name.span).to_string();
        let env = function_type_params(m.src, decl);
        let mut params = Vec::new();
        let mut variadic = None;
        let mut required = 0usize;
        let mut names = Vec::new();
        let mut defaulted = Vec::new();
        for (i, param) in decl.params.iter().enumerate() {
            let ty = former.form(&param.ty, &env);
            if param.variadic {
                if i + 1 != decl.params.len() {
                    former.error(
                        crate::codes::VARIADIC_POSITION,
                        "a variadic parameter must be final".to_string(),
                        param.span,
                    );
                }
                variadic = Some(Box::new(ty));
            } else {
                if param.default.is_none() {
                    required += 1;
                }
                names.push(text(m.src, param.name.span).to_string());
                defaulted.push(param.default.is_some());
                params.push(ty);
            }
        }
        let ret = match &decl.return_type {
            Some(ret) => former.form(ret, &env),
            None => Type::Unknown,
        };
        let ty = Type::Func {
            params,
            variadic,
            ret: Box::new(ret),
        };
        let mut nominals = ExportedNominals::default();
        former.collect_type_nominals(&ty, &mut nominals);
        surface.values.insert(
            name,
            ExportedValue {
                ty,
                vars: decl.type_params.len() as u32,
                bounds: function_bound_names(m.src, decl),
                required,
                names,
                names_known: true,
                defaulted,
                nominals,
            },
        );
    }
    (former.diagnostics, surface, Vec::new())
}

fn exported_main_decl<'a>(src: &str, program: &'a ast::Program) -> Option<&'a ast::FunctionDecl> {
    program.items.iter().find_map(|stmt| {
        let ast::StmtKind::Export(inner) = &stmt.kind else {
            return None;
        };
        let ast::StmtKind::Function(decl) = &inner.kind else {
            return None;
        };
        (text(src, decl.name.span) == "main").then_some(decl)
    })
}

fn explicit_main_type() -> Type {
    Type::Func {
        params: vec![
            Type::Ctor(Ctor::Array, vec![Type::Prim(Prim::String)]),
            Type::Prim(Prim::String),
        ],
        variadic: None,
        ret: Box::new(Type::Ctor(
            Ctor::Result,
            vec![Type::Prim(Prim::Int), Type::Prim(Prim::String)],
        )),
    }
}

fn validate_explicit_main_signature(
    m: &UnitModule<'_>,
    surface: &ModuleExports,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(decl) = exported_main_decl(m.src, m.program) else {
        return;
    };
    let expected = explicit_main_type();
    let Some(actual) = surface.values.get("main") else {
        return;
    };
    let exact_signature = actual.vars == 0
        && actual.required == 2
        && actual.defaulted == [false, false]
        && actual.ty == expected;
    if exact_signature {
        return;
    }
    diagnostics.push(Diagnostic::error(
        crate::codes::MALFORMED_TYPE,
        format!(
            "`export function main` must be non-generic and have signature `{expected}`; found `{}`",
            actual.ty
        ),
        Label::new(decl.name.span, ""),
    ));
}

fn merge_exported_nominals(
    nominals: &ExportedNominals,
    records: &mut HashMap<String, ExportedRecord>,
    enums: &mut HashMap<String, ExportedEnum>,
    newtypes: &mut HashMap<String, ExportedNewtype>,
) {
    for (identity, record) in &nominals.records {
        records
            .entry(identity.clone())
            .or_insert_with(|| record.clone());
    }
    for (identity, enumeration) in &nominals.enums {
        enums
            .entry(identity.clone())
            .or_insert_with(|| enumeration.clone());
    }
    for (identity, newtype) in &nominals.newtypes {
        newtypes
            .entry(identity.clone())
            .or_insert_with(|| newtype.clone());
    }
}

fn check_module(
    m: &UnitModule<'_>,
    exports: &HashMap<String, ModuleExports>,
    collect_typed: bool,
    version: LangVersion,
) -> CompletedModuleCheck {
    // Assemble the import surface of this module.
    let mut namespaces: HashMap<String, ModuleExports> = HashMap::new();
    let mut value_imports: Vec<(String, ExportedValue, Option<String>)> = Vec::new();
    let mut alias_imports: HashMap<String, ExportedAlias> = HashMap::new();
    let mut record_imports: HashMap<String, ExportedRecord> = HashMap::new();
    let mut enum_imports: HashMap<String, ExportedEnum> = HashMap::new();
    let mut newtype_imports: HashMap<String, ExportedNewtype> = HashMap::new();
    let mut imported_receiver_methods: HashMap<String, HashMap<String, ExportedReceiverMethod>> =
        HashMap::new();
    let mut imported_conformances: HashSet<(String, String)> = HashSet::new();
    let mut unexported: Vec<(String, topaz_diag::Span)> = Vec::new();
    let mut extern_imports: Vec<(String, String, topaz_diag::Span)> = Vec::new();
    let mut lispex_rule_factories: HashMap<String, String> = HashMap::new();
    let mut lispex_rule_namespaces: HashMap<String, HashMap<String, String>> = HashMap::new();

    // A selected value may precede its selected nominal in the same import
    // list, or the two may live in separate imports. Collect every target's
    // declaration-site -> local nominal spelling before binding any values.
    let mut selected_nominal_aliases: HashMap<String, HashMap<String, String>> = HashMap::new();
    for stmt in &m.program.items {
        let ast::StmtKind::Import(item) = &stmt.kind else {
            continue;
        };
        let ast::ImportKind::Selected { specs } = &item.kind else {
            continue;
        };
        let target = dotted(m.src, item);
        let Some(surface) = exports.get(&target) else {
            continue;
        };
        let aliases = selected_nominal_aliases.entry(target).or_default();
        for spec in specs {
            let name = text(m.src, spec.name.span);
            let bound = spec
                .alias
                .as_ref()
                .map(|alias| text(m.src, alias.span))
                .unwrap_or(name);
            let original = surface
                .records
                .get(name)
                .map(|record| record.id.as_str())
                .or_else(|| surface.enums.get(name).map(|enm| enm.id.as_str()))
                .or_else(|| {
                    surface
                        .newtypes
                        .get(name)
                        .map(|newtype| newtype.id.as_str())
                });
            if let Some(original) = original {
                aliases
                    .entry(original.to_string())
                    .or_insert_with(|| bound.to_string());
            }
        }
    }

    for stmt in &m.program.items {
        let ast::StmtKind::Import(item) = &stmt.kind else {
            continue;
        };
        let target = dotted(m.src, item);
        let surface = exports.get(&target).cloned().unwrap_or(ModuleExports {
            ambient: true,
            ..ModuleExports::default()
        });
        if surface.is_extern
            && let Some(error) = &surface.extern_replay_error
        {
            extern_imports.push((target.clone(), error.clone(), item.path.span));
        }
        match &item.kind {
            ast::ImportKind::Namespace { alias } => {
                let bound = match alias {
                    Some(a) => text(m.src, a.span).to_string(),
                    None => text(
                        m.src,
                        item.path.segments.last().expect("nonempty path").span,
                    )
                    .to_string(),
                };
                if !surface.ambient {
                    if version >= LangVersion::V5_20 {
                        imported_conformances.extend(surface.conformances.iter().cloned());
                    } else {
                        extend_imported_conformances(
                            &mut imported_conformances,
                            &surface.conformances,
                            |type_id| format!("{bound}.{type_id}"),
                        );
                    }
                }
                if version >= LangVersion::V5_20 {
                    for nominals in surface
                        .values
                        .values()
                        .map(|value| &value.nominals)
                        .chain(surface.aliases.values().map(|alias| &alias.nominals))
                        .chain(surface.records.values().map(|record| &record.nominals))
                        .chain(
                            surface
                                .enums
                                .values()
                                .map(|enumeration| &enumeration.nominals),
                        )
                        .chain(surface.newtypes.values().map(|newtype| &newtype.nominals))
                    {
                        merge_exported_nominals(
                            nominals,
                            &mut record_imports,
                            &mut enum_imports,
                            &mut newtype_imports,
                        );
                    }
                }
                namespaces.insert(bound.clone(), surface);
                if target == "std.lispex.rules"
                    && namespaces
                        .get(&bound)
                        .is_some_and(|surface| surface.is_generated_std)
                {
                    let members = namespaces[&bound]
                        .values
                        .keys()
                        .map(|name| (name.clone(), format!("topaz.lispex-rule-handle/v1:{name}")))
                        .collect();
                    lispex_rule_namespaces.insert(bound.clone(), members);
                }
            }
            ast::ImportKind::Selected { specs } => {
                if !surface.ambient {
                    if version >= LangVersion::V5_20 {
                        imported_conformances.extend(surface.conformances.iter().cloned());
                    } else {
                        let aliases = selected_nominal_aliases.get(&target);
                        extend_imported_conformances(
                            &mut imported_conformances,
                            &surface.conformances,
                            |type_id| {
                                aliases
                                    .and_then(|aliases| aliases.get(type_id))
                                    .cloned()
                                    .unwrap_or_else(|| type_id.to_string())
                            },
                        );
                    }
                }
                for spec in specs {
                    let name = text(m.src, spec.name.span);
                    let bound = match &spec.alias {
                        Some(a) => text(m.src, a.span).to_string(),
                        None => name.to_string(),
                    };
                    if let Some(v) = surface.values.get(name) {
                        let mut imported = v.clone();
                        if version < LangVersion::V5_20
                            && let Some(aliases) = selected_nominal_aliases.get(&target)
                        {
                            imported.ty = remap_nominal_identities(&imported.ty, aliases);
                        }
                        let target_identity = (target == "std.lispex.rules"
                            && surface.is_generated_std)
                            .then(|| format!("topaz.lispex-rule-handle/v1:{name}"));
                        if let Some(identity) = &target_identity {
                            lispex_rule_factories.insert(bound.clone(), identity.clone());
                        }
                        if version >= LangVersion::V5_20 {
                            merge_exported_nominals(
                                &imported.nominals,
                                &mut record_imports,
                                &mut enum_imports,
                                &mut newtype_imports,
                            );
                        }
                        value_imports.push((bound.clone(), imported, target_identity));
                    } else if let Some(a) = surface.aliases.get(name) {
                        if version >= LangVersion::V5_20 {
                            merge_exported_nominals(
                                &a.nominals,
                                &mut record_imports,
                                &mut enum_imports,
                                &mut newtype_imports,
                            );
                        }
                        alias_imports.insert(bound.clone(), a.clone());
                    } else if let Some(r) = surface.records.get(name) {
                        if version >= LangVersion::V5_20 {
                            merge_exported_nominals(
                                &r.nominals,
                                &mut record_imports,
                                &mut enum_imports,
                                &mut newtype_imports,
                            );
                        }
                        record_imports.insert(bound.clone(), r.clone());
                    } else if let Some(e) = surface.enums.get(name) {
                        if version >= LangVersion::V5_20 {
                            merge_exported_nominals(
                                &e.nominals,
                                &mut record_imports,
                                &mut enum_imports,
                                &mut newtype_imports,
                            );
                        }
                        enum_imports.insert(bound.clone(), e.clone());
                    } else if let Some(n) = surface.newtypes.get(name) {
                        if version >= LangVersion::V5_20 {
                            merge_exported_nominals(
                                &n.nominals,
                                &mut record_imports,
                                &mut enum_imports,
                                &mut newtype_imports,
                            );
                        }
                        newtype_imports.insert(bound.clone(), n.clone());
                    } else if !surface.ambient {
                        // A selected import binds a value OR a type alias, so both
                        // are candidate suggestions here.
                        let hint = topaz_diag::suggest::did_you_mean(
                            name,
                            surface
                                .values
                                .keys()
                                .chain(surface.aliases.keys())
                                .chain(surface.records.keys())
                                .chain(surface.enums.keys())
                                .chain(surface.newtypes.keys())
                                .map(String::as_str),
                        );
                        unexported.push((
                            format!("`{name}` is not exported by `{target}` (§17){hint}"),
                            spec.span,
                        ));
                    }
                    if let Some(methods) = surface.receiver_methods.get(name) {
                        imported_receiver_methods.insert(bound.clone(), methods.clone());
                    }
                }
            }
        }
    }

    let ns_aliases: HashMap<String, HashMap<String, ExportedAlias>> = namespaces
        .iter()
        .map(|(n, s)| (n.clone(), s.aliases.clone()))
        .collect();
    let ns_records: HashMap<String, HashMap<String, ExportedRecord>> = namespaces
        .iter()
        .map(|(n, s)| (n.clone(), s.records.clone()))
        .collect();
    let ns_enums: HashMap<String, HashMap<String, ExportedEnum>> = namespaces
        .iter()
        .map(|(n, s)| (n.clone(), s.enums.clone()))
        .collect();
    let ns_newtypes: HashMap<String, HashMap<String, ExportedNewtype>> = namespaces
        .iter()
        .map(|(n, s)| (n.clone(), s.newtypes.clone()))
        .collect();
    let namespace_receiver_methods = namespaces
        .iter()
        .map(|(n, s)| (n.clone(), s.receiver_methods.clone()))
        .collect();
    let ambient_namespaces: std::collections::HashSet<String> = namespaces
        .iter()
        .filter(|(_, s)| s.ambient)
        .map(|(n, _)| n.clone())
        .collect();

    let mut former = Former::with_module_context(
        m.src,
        m.program,
        version,
        ModuleContext {
            namespace_aliases: ns_aliases,
            namespace_records: ns_records,
            namespace_enums: ns_enums,
            namespace_newtypes: ns_newtypes,
            imported_aliases: alias_imports.clone(),
            imported_records: record_imports,
            imported_enums: enum_imports,
            imported_newtypes: newtype_imports,
            namespace_receiver_methods,
            imported_receiver_methods,
            ambient_namespaces,
            imported_conformances,
        },
    );
    former.set_receiver_method_dispatch_module(&m.identity);
    // Only derived conformances to globally predeclared builtin
    // protocols cross module boundaries. A name-only user/manual conformance
    // would otherwise collide with an unrelated same-spelled importer protocol,
    // while its definition and implementation body are not exported at all.
    let local_conformances = former.exportable_conformances();
    former.validate_aliases();
    let mut checker = ExprChecker::new(former);
    checker.enable_module_mode(namespaces);
    checker.enable_lispex_rule_factories(lispex_rule_factories, lispex_rule_namespaces);
    for (name, value) in generated_lispex_intrinsics(m, &alias_imports) {
        checker.bind_import(name, value);
    }
    if collect_typed {
        checker.enable_typed_locals();
    }
    for (name, v, _) in value_imports {
        checker.bind_import(name, v);
    }
    for (message, span) in unexported {
        checker.former.error(crate::codes::UNBOUND, message, span);
    }
    for (target, error, span) in extern_imports {
        checker.former.error(
            Code::new(topaz_diag::extern_codes::REPLAY),
            format!(
                "extern module `{target}` has an invalid deterministic replay binding: {error}"
            ),
            span,
        );
    }
    checker.check_items(&m.program.items);
    let defining_module = if m.is_entry { "" } else { m.identity.as_str() };
    let own_nominal_names = checker
        .former
        .own_nominal_names()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut local_aliases = checker
        .former
        .root_aliases(defining_module)
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let mut surface = checker.export_surface(m.program, defining_module);
    if version >= LangVersion::V5_20 {
        canonicalize_module_nominal_identities(
            defining_module,
            &own_nominal_names,
            &mut surface,
            &mut local_aliases,
        );
    }
    surface.is_generated_std = m.is_generated_std;
    if m.is_generated_std && m.identity == "std.lispex" {
        seal_generated_lispex_opaque_types(&mut surface);
        surface.enums.retain(|name, _| !name.starts_with("__"));
    }
    surface.conformances = if version >= LangVersion::V5_20 {
        local_conformances
            .iter()
            .map(|(protocol, type_id)| {
                (
                    protocol.clone(),
                    module_nominal_identity(defining_module, type_id),
                )
            })
            .collect()
    } else {
        local_conformances.clone()
    };
    let typed_locals = checker.take_typed_locals();
    let typed_nodes = checker.take_typed_nodes();
    let typed_call_targets = checker.take_typed_call_targets();
    let typed_call_callees = checker.take_typed_call_callees();
    CompletedModuleCheck {
        diagnostics: checker.former.diagnostics,
        surface,
        conformances: local_conformances,
        aliases: local_aliases,
        locals: typed_locals,
        nodes: typed_nodes,
        call_targets: typed_call_targets,
        call_callees: typed_call_callees,
    }
}

pub(crate) fn module_nominal_identity(module: &str, nominal: &str) -> String {
    let module = if module.is_empty() {
        "__entry__"
    } else {
        module
    };
    format!("{module}::{nominal}")
}

fn collect_nominal_identity_pairs(
    ty: &Type,
    local: &HashMap<String, String>,
    pairs: &mut HashMap<String, String>,
) {
    ty.for_each_component(|component| {
        if let Type::Enum { base, args }
        | Type::NominalRecord { base, args }
        | Type::Newtype { base, args } = component
        {
            let canonical = remap_nominal_identities(component, local);
            let canonical_id = match &canonical {
                Type::Enum { base, args }
                | Type::NominalRecord { base, args }
                | Type::Newtype { base, args } => nominal_instance_id(base, args),
                _ => unreachable!(),
            };
            pairs.insert(nominal_instance_id(base, args), canonical_id);
        }
    });
}

fn canonicalize_exported_nominals(
    nominals: &mut ExportedNominals,
    local: &HashMap<String, String>,
    pairs: &HashMap<String, String>,
) {
    let records = std::mem::take(&mut nominals.records);
    for (key, mut record) in records {
        record.id = pairs
            .get(&record.id)
            .cloned()
            .or_else(|| local.get(&record.id).cloned())
            .unwrap_or(record.id);
        for field in &mut record.fields {
            field.ty = remap_nominal_identities(&field.ty, local);
        }
        canonicalize_exported_nominals(&mut record.nominals, local, pairs);
        let key = pairs
            .get(&key)
            .cloned()
            .or_else(|| local.get(&key).cloned())
            .unwrap_or(key);
        nominals.records.insert(key, record);
    }
    let enums = std::mem::take(&mut nominals.enums);
    for (key, mut enumeration) in enums {
        enumeration.id = pairs
            .get(&enumeration.id)
            .cloned()
            .or_else(|| local.get(&enumeration.id).cloned())
            .unwrap_or(enumeration.id);
        for variant in &mut enumeration.variants {
            variant.payloads = variant
                .payloads
                .iter()
                .map(|payload| remap_nominal_identities(payload, local))
                .collect();
        }
        canonicalize_exported_nominals(&mut enumeration.nominals, local, pairs);
        let key = pairs
            .get(&key)
            .cloned()
            .or_else(|| local.get(&key).cloned())
            .unwrap_or(key);
        nominals.enums.insert(key, enumeration);
    }
    let newtypes = std::mem::take(&mut nominals.newtypes);
    for (key, mut newtype) in newtypes {
        newtype.id = pairs
            .get(&newtype.id)
            .cloned()
            .or_else(|| local.get(&newtype.id).cloned())
            .unwrap_or(newtype.id);
        newtype.base = remap_nominal_identities(&newtype.base, local);
        canonicalize_exported_nominals(&mut newtype.nominals, local, pairs);
        let key = pairs
            .get(&key)
            .cloned()
            .or_else(|| local.get(&key).cloned())
            .unwrap_or(key);
        nominals.newtypes.insert(key, newtype);
    }
}

fn canonicalize_module_nominal_identities(
    module: &str,
    own_nominal_names: &[String],
    surface: &mut ModuleExports,
    local_aliases: &mut BTreeMap<String, ExportedAlias>,
) {
    let local = own_nominal_names
        .iter()
        .map(|name| (name.clone(), module_nominal_identity(module, name)))
        .collect::<HashMap<_, _>>();
    let mut pairs = HashMap::new();
    for value in surface
        .values
        .values()
        .chain(surface.private_runtime_values.values())
    {
        collect_nominal_identity_pairs(&value.ty, &local, &mut pairs);
    }
    for alias in surface.aliases.values().chain(local_aliases.values()) {
        collect_nominal_identity_pairs(&alias.body, &local, &mut pairs);
    }
    for record in surface.records.values() {
        for field in &record.fields {
            collect_nominal_identity_pairs(&field.ty, &local, &mut pairs);
        }
    }
    for enumeration in surface.enums.values() {
        for variant in &enumeration.variants {
            for payload in &variant.payloads {
                collect_nominal_identity_pairs(payload, &local, &mut pairs);
            }
        }
    }
    for newtype in surface.newtypes.values() {
        collect_nominal_identity_pairs(&newtype.base, &local, &mut pairs);
    }
    for (name, record) in &mut surface.records {
        record.id = local
            .get(name)
            .cloned()
            .unwrap_or_else(|| record.id.clone());
        for field in &mut record.fields {
            field.ty = remap_nominal_identities(&field.ty, &local);
        }
        canonicalize_exported_nominals(&mut record.nominals, &local, &pairs);
    }
    for (name, enumeration) in &mut surface.enums {
        enumeration.id = local
            .get(name)
            .cloned()
            .unwrap_or_else(|| enumeration.id.clone());
        for variant in &mut enumeration.variants {
            variant.payloads = variant
                .payloads
                .iter()
                .map(|payload| remap_nominal_identities(payload, &local))
                .collect();
        }
        canonicalize_exported_nominals(&mut enumeration.nominals, &local, &pairs);
    }
    for (name, newtype) in &mut surface.newtypes {
        newtype.id = local
            .get(name)
            .cloned()
            .unwrap_or_else(|| newtype.id.clone());
        newtype.base = remap_nominal_identities(&newtype.base, &local);
        canonicalize_exported_nominals(&mut newtype.nominals, &local, &pairs);
    }
    for value in surface
        .values
        .values_mut()
        .chain(surface.private_runtime_values.values_mut())
    {
        value.ty = remap_nominal_identities(&value.ty, &local);
        canonicalize_exported_nominals(&mut value.nominals, &local, &pairs);
    }
    for alias in surface
        .aliases
        .values_mut()
        .chain(local_aliases.values_mut())
    {
        alias.body = remap_nominal_identities(&alias.body, &local);
        canonicalize_exported_nominals(&mut alias.nominals, &local, &pairs);
    }
    for (nominal, methods) in &mut surface.receiver_methods {
        let dispatch_id = local.get(nominal);
        for method in methods.values_mut() {
            if let Some(dispatch_id) = dispatch_id {
                method.dispatch_id = dispatch_id.clone();
            }
            method.info.params = method
                .info
                .params
                .iter()
                .map(|param| remap_nominal_identities(param, &local))
                .collect();
            method.info.variadic = method
                .info
                .variadic
                .as_ref()
                .map(|param| remap_nominal_identities(param, &local));
            method.info.ret = remap_nominal_identities(&method.info.ret, &local);
        }
    }
}

const PREPARED_LISPEX_RULE_TYPE_ID: &str = "topaz.internal/lispex-prepared-rule/1";
const LISPEX_VALUE_TYPE_ID: &str = "topaz.internal/lispex-value/1";
const LISPEX_CONSUMER_ARTIFACT_TYPE_ID: &str = "topaz.internal/lispex-consumer-artifact/1";

fn seal_generated_lispex_opaque_types(surface: &mut ModuleExports) {
    for alias in surface.aliases.values_mut() {
        alias.body = seal_lispex_type(&alias.body);
        seal_exported_nominals(&mut alias.nominals);
    }
    for value in surface
        .values
        .values_mut()
        .chain(surface.private_runtime_values.values_mut())
    {
        value.ty = seal_lispex_type(&value.ty);
        seal_exported_nominals(&mut value.nominals);
        value
            .nominals
            .enums
            .retain(|name, _| !name.starts_with("__"));
    }
    for record in surface.records.values_mut() {
        for field in &mut record.fields {
            field.ty = seal_lispex_type(&field.ty);
        }
        seal_exported_nominals(&mut record.nominals);
    }
    for enumeration in surface.enums.values_mut() {
        for variant in &mut enumeration.variants {
            variant.payloads = variant.payloads.iter().map(seal_lispex_type).collect();
        }
        seal_exported_nominals(&mut enumeration.nominals);
    }
    for newtype in surface.newtypes.values_mut() {
        newtype.base = seal_lispex_type(&newtype.base);
        seal_exported_nominals(&mut newtype.nominals);
    }
}

fn seal_exported_nominals(nominals: &mut ExportedNominals) {
    for record in nominals.records.values_mut() {
        for field in &mut record.fields {
            field.ty = seal_lispex_type(&field.ty);
        }
        seal_exported_nominals(&mut record.nominals);
    }
    for enumeration in nominals.enums.values_mut() {
        for variant in &mut enumeration.variants {
            variant.payloads = variant.payloads.iter().map(seal_lispex_type).collect();
        }
        seal_exported_nominals(&mut enumeration.nominals);
    }
    for newtype in nominals.newtypes.values_mut() {
        newtype.base = seal_lispex_type(&newtype.base);
        seal_exported_nominals(&mut newtype.nominals);
    }
}

fn seal_lispex_type(ty: &Type) -> Type {
    ty.transform_components(&mut |component| {
        let Type::Enum { base, args } = component else {
            return None;
        };
        let sealed = match base.as_str() {
            "__PreparedLispexRuleCarrier" => PREPARED_LISPEX_RULE_TYPE_ID,
            "__LispexValueCarrier" => LISPEX_VALUE_TYPE_ID,
            "__LispexConsumerArtifactCarrier" => LISPEX_CONSUMER_ARTIFACT_TYPE_ID,
            _ => return None,
        };
        Some(Type::Enum {
            base: sealed.to_string(),
            args: args.iter().map(seal_lispex_type).collect(),
        })
    })
}

fn generated_lispex_intrinsics(
    module: &UnitModule<'_>,
    imported_aliases: &HashMap<String, ExportedAlias>,
) -> Vec<(String, ExportedValue)> {
    if !module.is_generated_std {
        return Vec::new();
    }
    let enumeration = |base: &str| Type::Enum {
        base: base.to_string(),
        args: Vec::new(),
    };
    let record = |base: &str| Type::NominalRecord {
        base: base.to_string(),
        args: Vec::new(),
    };
    let result = |ok: Type, error: Type| Type::Ctor(Ctor::Result, vec![ok, error]);
    let function = |params: Vec<Type>, names: &[&str], ret: Type| ExportedValue {
        required: params.len(),
        names: names.iter().map(|name| (*name).to_string()).collect(),
        names_known: true,
        defaulted: vec![false; params.len()],
        ty: Type::Func {
            params,
            variadic: None,
            ret: Box::new(ret),
        },
        vars: 0,
        bounds: Vec::new(),
        nominals: ExportedNominals::default(),
    };
    match module.identity.as_str() {
        "std.lispex" => {
            let rule = enumeration("__PreparedLispexRuleCarrier");
            let value = enumeration("__LispexValueCarrier");
            let artifact = enumeration("__LispexConsumerArtifactCarrier");
            let value_error = enumeration("LispexValueError");
            let limits = record("LispexLimits");
            let identity = record("LispexRuleIdentity");
            let settlement = enumeration("LispexSettlement");
            let fault = enumeration("LispexOperationalFault");
            let evidence_outcome = enumeration("LispexEvidenceOutcome");
            let evidence_error = enumeration("LispexEvidenceError");
            let replay_error = enumeration("LispexReplayError");
            let inspection = record("LispexConsumerArtifactInspection");
            vec![
                (
                    "__lispexValueFromCanonical".to_string(),
                    function(
                        vec![Type::Bytes],
                        &["bytes"],
                        result(value.clone(), value_error),
                    ),
                ),
                (
                    "__lispexCanonicalBytes".to_string(),
                    function(vec![value.clone()], &["value"], Type::Bytes),
                ),
                (
                    "__lispexDefaultLimits".to_string(),
                    function(vec![rule.clone()], &["rule"], limits.clone()),
                ),
                (
                    "__lispexInspectRule".to_string(),
                    function(vec![rule.clone()], &["rule"], identity),
                ),
                (
                    "__lispexEvaluate".to_string(),
                    function(
                        vec![rule.clone(), value.clone(), limits.clone()],
                        &["rule", "input", "limits"],
                        result(settlement.clone(), fault.clone()),
                    ),
                ),
                (
                    "__lispexEvaluateWithEvidence".to_string(),
                    function(
                        vec![rule.clone(), value.clone(), limits],
                        &["rule", "input", "limits"],
                        result(evidence_outcome, fault),
                    ),
                ),
                (
                    "__lispexConsumerArtifactFromBytes".to_string(),
                    function(
                        vec![Type::Bytes],
                        &["bytes"],
                        result(artifact.clone(), evidence_error.clone()),
                    ),
                ),
                (
                    "__lispexConsumerArtifactBytes".to_string(),
                    function(vec![artifact.clone()], &["artifact"], Type::Bytes),
                ),
                (
                    "__lispexPortableCoreBytes".to_string(),
                    function(
                        vec![artifact.clone()],
                        &["artifact"],
                        result(Type::Bytes, evidence_error.clone()),
                    ),
                ),
                (
                    "__lispexInspectConsumerArtifact".to_string(),
                    function(
                        vec![artifact.clone()],
                        &["artifact"],
                        result(inspection.clone(), evidence_error.clone()),
                    ),
                ),
                (
                    "__lispexVerifyConsumerArtifact".to_string(),
                    function(
                        vec![artifact.clone()],
                        &["artifact"],
                        result(inspection, evidence_error),
                    ),
                ),
                (
                    "__lispexFreshReplay".to_string(),
                    function(
                        vec![rule, value, artifact],
                        &["rule", "input", "artifact"],
                        result(settlement, replay_error),
                    ),
                ),
            ]
        }
        "std.lispex.rules" => imported_aliases
            .get("PreparedLispexRule")
            .map(|alias| {
                vec![(
                    "__lispexRule".to_string(),
                    function(
                        vec![Type::Prim(Prim::String)],
                        &["name"],
                        alias.body.clone(),
                    ),
                )]
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
