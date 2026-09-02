use super::*;

impl<'a> ExprChecker<'a> {
    pub fn new(former: Former<'a>) -> Self {
        ExprChecker {
            former,
            scopes: vec![BindingScope::default()],
            pipe_value: None,
            collect_partial: false,
            partial_base: 0,
            hit_pending_ret: false,
            module_mode: false,
            namespaces: HashMap::new(),
            tyenv: vec![HashMap::new()],
            skolem_bounds: Vec::new(),
            ret_ctx: Vec::new(),
            ret_join: Vec::new(),
            loop_ctx: Vec::new(),
            skolem_counter: 0,
            projection_ids: Vec::new(),
            at_bare_binding: false,
            top_level: None,
            record_default_depth: 0,
            typed_locals: None,
            typed_nodes: None,
            typed_call_targets: None,
            typed_call_callees: None,
            typed_inference_solutions: HashMap::new(),
            lispex_rule_factories: HashMap::new(),
            lispex_rule_namespaces: HashMap::new(),
        }
    }

    /// Turn ON typed-HIR local collection (v5.4 native-emit substrate). After a
    /// clean check, `take_typed_locals` hands back the recorded `(name, span, Type)`
    /// list for conversion to `MonoTy`.
    pub fn enable_typed_locals(&mut self) {
        self.typed_locals = Some(Vec::new());
        self.typed_nodes = Some(HashMap::new());
        self.typed_call_targets = Some(Vec::new());
        self.typed_call_callees = Some(HashMap::new());
    }

    pub(crate) fn enable_lispex_rule_factories(
        &mut self,
        factories: HashMap<String, String>,
        namespaces: HashMap<String, HashMap<String, String>>,
    ) {
        self.lispex_rule_factories = factories;
        self.lispex_rule_namespaces = namespaces;
    }

    /// Take the collected typed locals (empty if collection was never enabled).
    pub fn take_typed_locals(&mut self) -> Vec<(String, Span, Type)> {
        let solutions = &self.typed_inference_solutions;
        self.typed_locals
            .take()
            .unwrap_or_default()
            .into_iter()
            .map(|(name, span, ty)| (name, span, resolve_inference(&ty, solutions)))
            .collect()
    }

    pub fn take_typed_nodes(&mut self) -> Vec<(topaz_hir::TypedNodeKind, Span, Type)> {
        self.typed_nodes
            .take()
            .unwrap_or_default()
            .into_iter()
            .map(|((kind, span), ty)| {
                (
                    kind,
                    span,
                    resolve_inference(&ty, &self.typed_inference_solutions),
                )
            })
            .collect()
    }

    pub fn take_typed_call_targets(&mut self) -> Vec<(Span, String)> {
        self.typed_call_targets.take().unwrap_or_default()
    }

    pub fn take_typed_call_callees(&mut self) -> Vec<(Span, Type)> {
        self.typed_call_callees
            .take()
            .unwrap_or_default()
            .into_iter()
            .map(|(span, ty)| {
                (
                    span,
                    resolve_inference(&ty, &self.typed_inference_solutions),
                )
            })
            .collect()
    }

    pub(super) fn record_typed_call_target(&mut self, span: Span, target: Option<String>) {
        let (Some(targets), Some(target)) = (self.typed_call_targets.as_mut(), target) else {
            return;
        };
        targets.push((span, target));
    }

    pub(super) fn record_typed_call_callee(&mut self, span: Span, ty: &Type) {
        let Some(callees) = self.typed_call_callees.as_mut() else {
            return;
        };
        callees.insert(span, ty.clone());
    }

    pub(super) fn record_typed_node(
        &mut self,
        kind: topaz_hir::TypedNodeKind,
        span: Span,
        ty: &Type,
    ) {
        let Some(nodes) = self.typed_nodes.as_mut() else {
            return;
        };
        nodes.insert((kind, span), ty.clone());
    }

    /// Retain concrete solutions for join-local inference variables. Partial
    /// variables are globally fresh, so one solution sharpens every fact carrying
    /// that identity when the typed product leaves this checker.
    pub(super) fn resolve_recorded_inference(&mut self, solutions: &[(u32, Type)]) {
        if self.typed_nodes.is_none() {
            return;
        }
        for (index, solution) in solutions {
            self.typed_inference_solutions
                .insert(*index, solution.clone());
        }
    }

    pub(super) fn resolve_recorded_dense_inference(
        &mut self,
        index: &[u32],
        subst: &[Option<Type>],
    ) {
        let mut solutions = Vec::with_capacity(index.len());
        for (dense_index, original_index) in index.iter().enumerate() {
            let mut solution = substitute(&Type::Var(dense_index as u32), subst);
            for _ in 0..subst.len() {
                let next = substitute(&solution, subst);
                if next == solution {
                    break;
                }
                solution = next;
            }
            if !type_has_var(&solution) {
                solutions.push((*original_index, solution));
            }
        }
        self.resolve_recorded_inference(&solutions);
    }

    /// Solve a partial expression type from a concrete peer context and carry
    /// that solution into the facts recorded while the partial was inferred.
    pub(super) fn solve_recorded_inference_against(
        &mut self,
        partial: &Type,
        concrete: &Type,
    ) -> Type {
        let mut index = Vec::new();
        collect_vars_into(partial, &mut index);
        if index.is_empty() {
            return partial.clone();
        }
        let dense = remap_vars(partial, &index);
        let mut subst = vec![None; index.len()];
        unify_with(&dense, concrete, &mut subst, false);
        let mut resolved = substitute(&dense, &subst);
        for _ in 0..subst.len() {
            let next = substitute(&resolved, &subst);
            if next == resolved {
                break;
            }
            resolved = next;
        }
        if type_has_var(&resolved) {
            return partial.clone();
        }
        self.resolve_recorded_dense_inference(&index, &subst);
        resolved
    }

    /// Record one declared local's `(name, span, Type)` — a `let`/`const` binding
    /// or a function parameter — when typed-HIR collection is enabled. A no-op on
    /// the ordinary check path.
    pub(super) fn record_typed_local(&mut self, name: &str, span: Span, ty: &Type) {
        if let Some(locals) = self.typed_locals.as_mut() {
            locals.push((name.to_string(), span, ty.clone()));
        }
    }

    pub(super) fn lookup(&self, name: &str) -> Option<&Type> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.bindings.get(name))
    }

    /// Callable metadata at the scope holding `name`'s innermost
    /// BINDING — a shadowing binding without metadata hides an
    /// outer declaration's (§4).
    pub(super) fn fn_meta_of(&self, name: &str) -> Option<&FnMeta> {
        self.scopes
            .iter()
            .rev()
            .find(|scope| scope.bindings.contains_key(name))?
            .fn_meta
            .get(name)
    }

    /// A generic function used as a VALUE (most importantly as a callback) is
    /// instantiated against the expected function shape. This keeps the callee
    /// scheme's inference vars and the callback's own type params from sharing the
    /// same small `Var(0)` namespace, so `xs.sortedBy(id)` can solve the real key
    /// type from `id<T>(x: T) -> T`.
    pub(super) fn instantiate_generic_value_against(
        &mut self,
        name: &str,
        ty: &Type,
        expected: &Type,
        span: Span,
    ) -> Option<Type> {
        let meta = self.fn_meta_of(name)?.clone();
        if meta.vars == 0 {
            return None;
        }
        let Type::Func {
            params,
            variadic,
            ret,
        } = ty
        else {
            return None;
        };
        let Type::Func {
            params: expected_params,
            variadic: expected_variadic,
            ret: expected_ret,
        } = expected
        else {
            return None;
        };
        if params.len() != expected_params.len() {
            return None;
        }

        let mut subst: Vec<Option<Type>> = vec![None; meta.vars as usize];
        for (param, expected_param) in params.iter().zip(expected_params.iter()) {
            unify_with(param, expected_param, &mut subst, false);
        }
        if let (Some(param), Some(expected_param)) =
            (variadic.as_deref(), expected_variadic.as_deref())
        {
            unify_with(param, expected_param, &mut subst, false);
        }
        if !type_has_var(expected_ret) {
            unify_with(ret, expected_ret, &mut subst, false);
        }
        if !self.check_protocol_bounds(&meta.bounds, &subst, span) {
            return Some(Type::Unknown);
        }

        let instantiated = Type::Func {
            params: params.iter().map(|p| substitute(p, &subst)).collect(),
            variadic: variadic.as_ref().map(|v| Box::new(substitute(v, &subst))),
            ret: Box::new(substitute(ret, &subst)),
        };
        Some(if type_has_var(&instantiated) {
            unknown_for_vars(&instantiated)
        } else {
            instantiated
        })
    }

    pub(super) fn record_fn_meta(&mut self, name: String, meta: FnMeta) {
        self.scopes
            .last_mut()
            .expect("scope stack non-empty")
            .fn_meta
            .insert(name, meta);
    }

    pub(super) fn bind(&mut self, name: String, ty: Type) {
        self.scopes
            .last_mut()
            .expect("scope stack non-empty")
            .bindings
            .insert(name, ty);
    }

    /// A declaration-site bind: same-scope redeclaration is a static
    /// error (§4; runtime guard TPZ5008 graduates for locals).
    pub(super) fn bind_decl(&mut self, name: String, ty: Type, mutable: bool, span: Span) {
        if self
            .scopes
            .last()
            .expect("scope stack non-empty")
            .bindings
            .contains_key(&name)
        {
            self.former.error(
                codes::REDECLARE,
                format!("`{name}` is already declared in this scope"),
                span,
            );
        }
        self.bind_with_mut(name, ty, mutable);
    }

    pub(super) fn bind_with_mut(&mut self, name: String, ty: Type, mutable: bool) {
        if mutable {
            self.scopes
                .last_mut()
                .expect("scope stack non-empty")
                .mutable
                .insert(name.clone());
        }
        self.bind(name, ty);
    }

    pub(super) fn push_scope(&mut self) {
        self.scopes.push(BindingScope::default());
    }

    pub(super) fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Whether `name` resolves (innermost-first) to a binding whose
    /// omitted return type is still pending — directly, or through
    /// alias links consulted live against the source binding.
    pub(super) fn resolves_to_pending<'scope>(&'scope self, name: &'scope str) -> bool {
        let mut current = name;
        let mut from = self.scopes.len();
        // Cycle guard by visited bindings: each step visits a new
        // (scope, name) pair, so arbitrarily long VALID alias chains
        // resolve while cycles terminate.
        let mut visited: HashSet<(usize, &str)> = HashSet::new();
        loop {
            let mut found = None;
            for i in (0..from).rev() {
                if self.scopes[i].bindings.contains_key(current) {
                    found = Some(i);
                    break;
                }
            }
            let Some(i) = found else { return false };
            if !visited.insert((i, current)) {
                return false;
            }
            if self.scopes[i].pending_returns.contains(current) {
                return true;
            }
            match self.scopes[i].pending_links.get(current) {
                Some((src_scope, src_name)) => {
                    // Follow the link: re-resolve the source from
                    // its recorded binding scope.
                    from = src_scope + 1;
                    current = src_name;
                }
                None => return false,
            }
        }
    }

    /// The scope index that binds `name` (innermost-first).
    pub(super) fn binding_scope(&self, name: &str) -> Option<usize> {
        (0..self.scopes.len())
            .rev()
            .find(|&i| self.scopes[i].bindings.contains_key(name))
    }

    /// Clears the pending marker on the scope that BINDS `name`.
    pub(super) fn clear_pending(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if scope.bindings.contains_key(name) {
                scope.pending_returns.remove(name);
                return;
            }
        }
    }

    /// The innermost declaration decides: Some(mutable) when the
    /// name is bound, None when it is ambient.
    /// §9: an in-place collection mutator (`recv.push(..)` etc.)
    /// requires its receiver's root binding to be `let mut`.
    pub(super) fn check_mutation_root(&mut self, receiver: &ast::Expr) {
        if let Some(root) = assignment_root(receiver) {
            let name = self.former.text(root.span);
            if self.mutability(name) == Some(false) {
                self.former.error(
                    codes::IMMUTABLE,
                    format!(
                        "`{name}` is not `let mut`; in-place collection mutation requires a mutable binding (§9)"
                    ),
                    root.span,
                );
            }
        }
    }

    /// §9 at a `recv.field` access (call OR value): when `field` is
    /// a mutator on the collection `object_ty`, the receiver's root
    /// binding must be `let mut`.
    pub(super) fn check_mutator_access(
        &mut self,
        object: &ast::Expr,
        object_ty: &Type,
        field: &ast::Ident,
    ) {
        let member = self.former.text(field.span);
        // A mutator handle acquired on a receiver that may BE a concrete arm
        // (`Array<int> | T`) still needs a mutable root (§9), exactly as the
        // member-CALL path does via the same predicate.
        if builtins::is_mutator(object_ty, member) || union_arm_is_mutator(object_ty, member) {
            self.check_mutation_root(object);
        }
    }

    pub(super) fn mutability(&self, name: &str) -> Option<bool> {
        for scope in self.scopes.iter().rev() {
            if scope.bindings.contains_key(name) {
                return Some(scope.mutable.contains(name));
            }
        }
        None
    }

    pub(super) fn tyenv(&self) -> HashMap<&'a str, Type> {
        self.tyenv.last().expect("tyenv stack non-empty").clone()
    }

    pub(super) fn stable_type_parameter_substitutions(
        &self,
        parameters: &[ast::Ident],
    ) -> Vec<Option<Type>> {
        parameters
            .iter()
            .enumerate()
            .map(|(index, parameter)| {
                Some(Type::Skolem {
                    name: self.former.text(parameter.span).to_string(),
                    id: u32::MAX - index as u32,
                    origin: format!(
                        "source:{}:{}:{}",
                        parameter.span.file.0, parameter.span.lo, parameter.span.hi
                    ),
                })
            })
            .collect()
    }

    pub(super) fn type_param_bound_names(&self, decl: &ast::FunctionDecl) -> Vec<Vec<String>> {
        (0..decl.type_params.len())
            .map(|i| {
                decl.type_param_bounds
                    .get(i)
                    .map(|bounds| {
                        bounds
                            .iter()
                            .map(|bound| self.former.text(bound.span).to_string())
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    /// Enters module-aware mode with this module's namespace
    /// bindings (CDR-004 C-6).
    pub(crate) fn enable_module_mode(&mut self, namespaces: HashMap<String, ModuleExports>) {
        self.module_mode = true;
        self.namespaces = namespaces;
    }

    /// Binds a selected import; callable metadata feeds call typing.
    pub(crate) fn bind_import(&mut self, name: String, v: ExportedValue) {
        if matches!(v.ty, Type::Func { .. }) {
            self.record_fn_meta(
                name.clone(),
                FnMeta {
                    vars: v.vars,
                    bounds: v.bounds.clone(),
                    required: v.required,
                    // None keeps an exported lambda's named
                    // arguments unjudged rather than hard-erroring.
                    names: v.names_known.then(|| v.names.clone()),
                    defaulted: v.defaulted.clone(),
                },
            );
        }
        self.bind(name, v.ty);
    }

    /// The published type of an exported binding, guarded against a
    /// leaked projection: a rigid `FieldOf<T, x>` &c has no nameable
    /// spelling, so it must not escape the module surface. If one is
    /// present, demand an annotation (TPZ5022) and gradualize it out.
    pub(super) fn export_value_ty(&mut self, name: &str, span: Span) -> Type {
        let ty = self.top_binding(name).unwrap_or(Type::Unknown);
        if contains_projection(&ty, &self.projection_ids) {
            self.former.error(
                codes::MALFORMED_TYPE,
                format!(
                    "cannot infer a type for exported `{name}`: it exposes a destructured generic value with no nameable type — annotate it"
                ),
                span,
            );
            strip_projections(&ty, &self.projection_ids)
        } else {
            ty
        }
    }

    /// A namespace-private runtime value is metadata for record-default
    /// re-elaboration, not a public export. Never issue an export diagnostic
    /// merely for declaring it locally, and never let a module-local projection
    /// skolem cross the module boundary. The taint bit makes a later namespace
    /// lookup reject explicitly rather than treating the gradualized type as a
    /// proof that the original value was compatible.
    pub(super) fn private_runtime_value_ty(&self, name: &str) -> (Type, bool) {
        let ty = self
            .top_binding(name)
            .expect("top-level immutable let must be bound before surface collection");
        let projection_tainted = contains_projection(&ty, &self.projection_ids);
        let ty = if projection_tainted {
            strip_projections(&ty, &self.projection_ids)
        } else {
            ty
        };
        (ty, projection_tainted)
    }

    pub(super) fn export_value_nominals(&self, ty: &Type) -> ExportedNominals {
        let mut nominals = ExportedNominals::default();
        self.former.collect_type_nominals(ty, &mut nominals);
        nominals
    }

    /// The module's export surface, read off the checked top scope
    /// (§17: signatures checked at the defining module).
    pub(crate) fn export_surface(
        &mut self,
        program: &ast::Program,
        module_identity: &str,
    ) -> ModuleExports {
        let mut surface = ModuleExports::default();
        for stmt in &program.items {
            let ast::StmtKind::Export(inner) = &stmt.kind else {
                continue;
            };
            match &inner.kind {
                ast::StmtKind::Function(decl) => {
                    let name = self.former.text(decl.name.span);
                    let ty = self.export_value_ty(name, decl.name.span);
                    let meta = self
                        .scopes
                        .first()
                        .and_then(|scope| scope.fn_meta.get(name));
                    let nominals = self.export_value_nominals(&ty);
                    surface.values.insert(
                        name.to_string(),
                        ExportedValue {
                            ty,
                            vars: meta.map_or(0, |m| m.vars),
                            bounds: meta.map(|m| m.bounds.clone()).unwrap_or_default(),
                            required: meta.map_or(0, |m| m.required),
                            names: meta.and_then(|m| m.names.clone()).unwrap_or_default(),
                            names_known: meta.is_some_and(|m| m.names.is_some()),
                            defaulted: meta.map(|m| m.defaulted.clone()).unwrap_or_default(),
                            nominals,
                        },
                    );
                }
                ast::StmtKind::Let { pattern, .. } => {
                    if let ast::PatternKind::Binding(name) | ast::PatternKind::Typed { name, .. } =
                        &pattern.kind
                    {
                        let span = name.span;
                        let name = self.former.text(span);
                        let ty = self.export_value_ty(name, span);
                        let meta = self
                            .scopes
                            .first()
                            .and_then(|scope| scope.fn_meta.get(name));
                        let nominals = self.export_value_nominals(&ty);
                        let required = meta.map(|m| m.required).unwrap_or(match &ty {
                            Type::Func { params, .. } => params.len(),
                            _ => 0,
                        });
                        surface.values.insert(
                            name.to_string(),
                            ExportedValue {
                                ty,
                                vars: meta.map_or(0, |m| m.vars),
                                bounds: meta.map(|m| m.bounds.clone()).unwrap_or_default(),
                                required,
                                names: meta.and_then(|m| m.names.clone()).unwrap_or_default(),
                                names_known: meta.is_some_and(|m| m.names.is_some()),
                                defaulted: meta.map(|m| m.defaulted.clone()).unwrap_or_default(),
                                nominals,
                            },
                        );
                    }
                }
                ast::StmtKind::Const { name, .. } => {
                    let span = name.span;
                    let name = self.former.text(span);
                    let ty = self.export_value_ty(name, span);
                    let meta = self
                        .scopes
                        .first()
                        .and_then(|scope| scope.fn_meta.get(name));
                    let nominals = self.export_value_nominals(&ty);
                    let required = meta.map(|m| m.required).unwrap_or(match &ty {
                        Type::Func { params, .. } => params.len(),
                        _ => 0,
                    });
                    surface.values.insert(
                        name.to_string(),
                        ExportedValue {
                            ty,
                            vars: meta.map_or(0, |m| m.vars),
                            bounds: meta.map(|m| m.bounds.clone()).unwrap_or_default(),
                            required,
                            names: meta.and_then(|m| m.names.clone()).unwrap_or_default(),
                            names_known: meta.is_some_and(|m| m.names.is_some()),
                            defaulted: meta.map(|m| m.defaulted.clone()).unwrap_or_default(),
                            nominals,
                        },
                    );
                }
                ast::StmtKind::TypeAlias(alias) => {
                    let name = self.former.text(alias.name.span);
                    if let Some((params, body)) = self.former.exported_alias(name) {
                        let mut nominals = ExportedNominals::default();
                        self.former.collect_type_nominals(&body, &mut nominals);
                        surface.aliases.insert(
                            name.to_string(),
                            ExportedAlias {
                                defining_module: module_identity.to_string(),
                                params,
                                body,
                                nominals,
                            },
                        );
                    }
                }
                ast::StmtKind::Record(decl) => {
                    let name = self.former.text(decl.name.span).to_string();
                    if let Some((id, params, fields)) = self.former.record_info(&name).map(|info| {
                        (
                            info.id.clone(),
                            info.type_params.len(),
                            info.fields
                                .iter()
                                .map(|field| ExportedRecordField {
                                    name: field.name.clone(),
                                    ty: field.ty.clone(),
                                    has_default: field.has_default,
                                })
                                .collect::<Vec<_>>(),
                        )
                    }) {
                        let mut nominals = ExportedNominals::default();
                        for field in &fields {
                            self.former.collect_type_nominals(&field.ty, &mut nominals);
                        }
                        surface.records.insert(
                            name,
                            ExportedRecord {
                                id,
                                params,
                                fields,
                                nominals,
                            },
                        );
                    }
                }
                ast::StmtKind::Enum(decl) => {
                    let name = self.former.text(decl.name.span).to_string();
                    if let Some((id, params, variants)) = self.former.enum_info(&name).map(|info| {
                        (
                            info.id.clone(),
                            info.type_params.len(),
                            info.variants
                                .iter()
                                .map(|variant| ExportedEnumVariant {
                                    name: variant.name.clone(),
                                    payloads: variant.payloads.clone(),
                                })
                                .collect::<Vec<_>>(),
                        )
                    }) {
                        let mut nominals = ExportedNominals::default();
                        for variant in &variants {
                            for payload in &variant.payloads {
                                self.former.collect_type_nominals(payload, &mut nominals);
                            }
                        }
                        surface.enums.insert(
                            name,
                            ExportedEnum {
                                id,
                                params,
                                variants,
                                nominals,
                            },
                        );
                    }
                }
                ast::StmtKind::Newtype(decl) => {
                    let name = self.former.text(decl.name.span).to_string();
                    if let Some((id, params, base)) = self
                        .former
                        .newtype_info(&name)
                        .map(|info| (info.id.clone(), info.type_params.len(), info.base.clone()))
                    {
                        let mut nominals = ExportedNominals::default();
                        self.former.collect_type_nominals(&base, &mut nominals);
                        surface.newtypes.insert(
                            name,
                            ExportedNewtype {
                                id,
                                params,
                                base,
                                nominals,
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        // Exported receiver methods accompany an exported nominal; they do not
        // create runtime/value exports of their own. An exported method on a
        // private nominal therefore remains private.
        for stmt in &program.items {
            let inner = match &stmt.kind {
                ast::StmtKind::Export(inner) => &**inner,
                _ => stmt,
            };
            let ast::StmtKind::Impl(decl) = &inner.kind else {
                continue;
            };
            if decl.target.is_some() {
                continue;
            }
            let nominal = self.former.text(decl.name.span).to_string();
            let nominal_exported = surface.records.contains_key(&nominal)
                || surface.enums.contains_key(&nominal)
                || surface.newtypes.contains_key(&nominal);
            if !nominal_exported {
                continue;
            }
            for method in &decl.methods {
                if !method.exported {
                    continue;
                }
                let method_name = self.former.text(method.decl.name.span).to_string();
                if let Some(info) = self.former.method_info(&nominal, &method_name).cloned() {
                    surface
                        .receiver_methods
                        .entry(nominal.clone())
                        .or_default()
                        .insert(
                            method_name,
                            ExportedReceiverMethod {
                                dispatch_id: format!("{module_identity}::{nominal}"),
                                info,
                            },
                        );
                }
            }
        }
        for stmt in &program.items {
            if matches!(&stmt.kind, ast::StmtKind::Export(_)) {
                continue;
            }
            let ast::StmtKind::Let {
                mutable, pattern, ..
            } = &stmt.kind
            else {
                continue;
            };
            if *mutable {
                continue;
            }
            if let ast::PatternKind::Binding(name) | ast::PatternKind::Typed { name, .. } =
                &pattern.kind
            {
                let span = name.span;
                let name = self.former.text(span);
                let (ty, projection_tainted) = self.private_runtime_value_ty(name);
                let meta = self
                    .scopes
                    .first()
                    .and_then(|scope| scope.fn_meta.get(name));
                let nominals = self.export_value_nominals(&ty);
                let required = meta.map(|m| m.required).unwrap_or(match &ty {
                    Type::Func { params, .. } => params.len(),
                    _ => 0,
                });
                surface.private_runtime_values.insert(
                    name.to_string(),
                    ExportedValue {
                        ty,
                        vars: meta.map_or(0, |m| m.vars),
                        bounds: meta.map(|m| m.bounds.clone()).unwrap_or_default(),
                        required,
                        names: meta.and_then(|m| m.names.clone()).unwrap_or_default(),
                        names_known: meta.is_some_and(|m| m.names.is_some()),
                        defaulted: meta.map(|m| m.defaulted.clone()).unwrap_or_default(),
                        nominals,
                    },
                );
                if projection_tainted {
                    surface
                        .private_runtime_projection_tainted
                        .insert(name.to_string());
                }
            }
        }
        surface
    }

    /// `let g = f` aliases a function value: the callable metadata
    /// (rank-1 vars, required arity incl. defaults) follows the new
    /// name so calls and re-exports stay faithful.
    pub(super) fn propagate_fn_meta(&mut self, value: &ast::Expr, bound: &str) {
        if let Some(span) = alias_source(value) {
            let source = self.former.text(span);
            if let Some(meta) = self.fn_meta_of(source).cloned() {
                self.record_fn_meta(bound.to_string(), meta);
            }
            // Pending-ness follows the alias LIVE: the alias points
            // at the source binding's scope, so it stops tainting
            // the moment the source completes (CDR-004 §7).
            if let Some(src_scope) = self.binding_scope(source) {
                let link = (src_scope, source.to_string());
                self.scopes
                    .last_mut()
                    .expect("pending stack non-empty")
                    .pending_links
                    .insert(bound.to_string(), link);
            }
        }
    }

    /// A lambda body under its OWN return context (§5/§7: `return`
    /// belongs to the innermost function or lambda), joining `return`
    /// statements with the body value.
    pub(super) fn lambda_body_type(&mut self, body: &'a ast::Expr) -> Type {
        self.ret_ctx.push(None);
        self.ret_join.push(Vec::new());
        // A lambda body has its own loop context — loop control may not
        // cross a function/lambda boundary, so an enclosing loop is hidden while
        // the body is checked (a `break` inside the lambda is "outside a loop").
        let saved_loop_ctx = std::mem::take(&mut self.loop_ctx);
        // Returns may mutually complete (`Ok`/`Err` pairs, §22.1):
        // collect partials for the join solver.
        let saved_collect = self.collect_partial;
        self.collect_partial = true;
        let body_ty = self.infer(body);
        self.collect_partial = saved_collect;
        self.loop_ctx = saved_loop_ctx;
        let collected = self.ret_join.pop().expect("ret_join stack");
        self.ret_ctx.pop();
        if collected.is_empty() {
            return body_ty;
        }
        let mut members = collected;
        if !arm_diverges(body) {
            members.push(body_ty);
        }
        self.join_branches(members, None, false, body.span)
    }

    pub(super) fn top_binding(&self, name: &str) -> Option<Type> {
        self.scopes.first()?.bindings.get(name).cloned()
    }

    /// A "; did you mean `X`?" suffix for an UNBOUND name (TPZ5002) in VALUE
    /// position, drawn from names that are usable AS A VALUE: locals and the
    /// closed unit's top level (all scope frames), the builtin free functions,
    /// and the nullary builtin constants (`None`). Imported namespaces are NOT
    /// offered — a bare namespace is not a value (TPZ3012), so suggesting one
    /// would just lead to another diagnostic. The pure suggestion logic lives in
    /// `topaz_diag::suggest`.
    pub(super) fn unbound_hint(&self, name: &str) -> String {
        let scope_names = self
            .scopes
            .iter()
            .flat_map(|scope| scope.bindings.keys().map(String::as_str));
        let free = builtins::FREE_FUNCTION_NAMES.iter().copied();
        let consts = builtins::CONSTANT_NAMES.iter().copied();
        topaz_diag::suggest::did_you_mean(name, scope_names.chain(free).chain(consts))
    }

    /// Like `unbound_hint`, but for a CALLEE position (`foo(…)`): a candidate is
    /// offered only if the name it would actually RESOLVE to is callable. Name
    /// lookup is innermost-first, so a candidate must be the EFFECTIVE binding of
    /// its name and that binding must be `Type::Func`; a builtin free function is
    /// a candidate only when no visible binding shadows it. (A function shadowed
    /// by a non-callable local, or a builtin shadowed by a local value, is not
    /// callable at the use site and must not be suggested.)
    pub(super) fn unbound_callee_hint(&self, name: &str) -> String {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut candidates: Vec<&str> = Vec::new();
        for scope in self.scopes.iter().rev() {
            for (k, t) in &scope.bindings {
                let k = k.as_str();
                // First (innermost) sighting is the effective binding for `k`.
                if seen.insert(k) && matches!(t, Type::Func { .. }) {
                    candidates.push(k);
                }
            }
        }
        for f in builtins::FREE_FUNCTION_NAMES {
            if !seen.contains(*f) {
                candidates.push(f);
            }
        }
        topaz_diag::suggest::did_you_mean(name, candidates)
    }

    /// Member access through a namespace binding: the exported
    /// value's type, TPZ5002 when not exported, silence when the
    /// namespace is ambient (cycles).
    pub(super) fn namespace_member(&mut self, ns: &str, field: &ast::Ident) -> Type {
        let member = self.former.text(field.span).to_string();
        let surface = self.namespaces.get(ns).expect("namespace checked");
        if surface.ambient {
            return Type::Unknown;
        }
        match surface.values.get(&member) {
            Some(v) => self.former.qualify_namespace_type(ns, &v.ty),
            None => {
                if self.record_default_depth > 0
                    && let Some(v) = surface.private_runtime_values.get(&member)
                {
                    if surface.private_runtime_projection_tainted.contains(&member) {
                        self.former.error(
                            codes::MALFORMED_TYPE,
                            format!(
                                "namespace-private `{member}` exposes a destructured generic value with no nameable type and cannot be used as a record default — annotate it"
                            ),
                            field.span,
                        );
                        return Type::Unknown;
                    }
                    return self.former.qualify_namespace_type(ns, &v.ty);
                }
                let hint = topaz_diag::suggest::did_you_mean(
                    &member,
                    surface.values.keys().map(String::as_str),
                );
                self.former.error(
                    codes::UNBOUND,
                    format!("`{member}` is not exported by the module bound as `{ns}` (§17){hint}"),
                    field.span,
                );
                Type::Unknown
            }
        }
    }

    pub fn check_items(&mut self, items: &'a [ast::Stmt]) {
        // The interpreter binds a module's top level in a fixed order
        // (`topaz_interp::machine`): a load-time CONST PASS evaluates
        // every `const` in textual order FIRST (each const sees only
        // earlier consts), then the remaining statements run in textual
        // order. Type visibility mirrors that: hoist every signature,
        // HOIST every const type up front (so a body or a later
        // statement reading a textually-later const types against it,
        // matching the runtime), then check items in textual order.
        self.top_level = Some(crate::forward::TopLevel::build(items, self.former.source()));
        self.hoist_functions(items);
        self.hoist_consts(items);
        for stmt in items {
            // Consts are already typed and bound by `hoist_consts`.
            if !matches!(top_inner(stmt), ast::StmtKind::Const { .. }) {
                self.check_stmt(stmt);
            }
        }
        // INIT ORDER (§4): a top-level NON-function statement must not,
        // in its OWN immediately-evaluated expression, reach a
        // `let`/`function` not yet bound at that point. A reference from
        // a short-circuit RHS, optional-call argument, conditional branch,
        // or function/lambda/defer body to a later binding is §4-allowed
        // (mutual recursion); evaluating it early is a dynamic fault, not a
        // static error.
        self.check_init_order(items);
    }

    /// Binds every top-level `const`'s type up front, in textual order,
    /// so each const sees only EARLIER consts — the interpreter's
    /// load-time const pass. A const reading a LATER const finds it
    /// unbound here, exactly as the runtime const pass faults.
    pub(super) fn hoist_consts(&mut self, items: &'a [ast::Stmt]) {
        for stmt in items {
            if matches!(top_inner(stmt), ast::StmtKind::Const { .. }) {
                self.check_stmt(stmt);
            }
        }
    }

    /// Reports forward references (§4) for the IMMEDIATELY-EVALUATED
    /// expression of each top-level non-function statement. Type
    /// visibility is already established; this is purely runtime
    /// availability, scanned syntactically — it never follows a call
    /// into another body.
    pub(super) fn check_init_order(&mut self, items: &'a [ast::Stmt]) {
        let Some(top) = self.top_level.take() else {
            return;
        };
        for (index, stmt) in items.iter().enumerate() {
            match top_inner(stmt) {
                ast::StmtKind::Let { value, .. } => {
                    top.check_item(index, value, &mut self.former);
                }
                ast::StmtKind::Using { value, body, .. } => {
                    top.check_item(index, value, &mut self.former);
                    top.check_block(index, body, &mut self.former);
                }
                // A function declaration only binds a closure; its body
                // is delayed (§4-allowed). A const's initializer sees
                // only earlier consts and is already gated by the const
                // pass above. A `defer` body is delayed — it runs at
                // scope exit, after every top-level binding is bound.
                ast::StmtKind::Function(_)
                | ast::StmtKind::Const { .. }
                | ast::StmtKind::Defer(_)
                | ast::StmtKind::TypeAlias(_)
                | ast::StmtKind::Enum(_)
                | ast::StmtKind::Record(_)
                | ast::StmtKind::Newtype(_)
                // §4 (v5.4) impl methods only bind delayed bodies (like functions);
                // their bodies run only when the method is called.
                | ast::StmtKind::Impl(_)
                // §4 (v5.4) a protocol declaration binds no value; signatures only.
                | ast::StmtKind::Protocol(_)
                | ast::StmtKind::Import(_)
                // §17: a `break`/`continue` is loop-internal; a top-level one is
                // a static error caught in `check_stmt`. The `break <value>`
                // value (if any) is a delayed/repeated body like a loop body —
                // not scanned in the immediate init-order pass.
                | ast::StmtKind::Break { .. }
                | ast::StmtKind::Continue { .. } => {}
                ast::StmtKind::Expr(e) => top.check_item(index, e, &mut self.former),
                ast::StmtKind::Return(Some(e)) => top.check_item(index, e, &mut self.former),
                ast::StmtKind::Return(None) => {}
                ast::StmtKind::Assign { target, value, .. } => {
                    top.check_item(index, target, &mut self.former);
                    top.check_item(index, value, &mut self.former);
                }
                ast::StmtKind::While { cond, .. } => {
                    // The condition is immediate; the loop body is a
                    // delayed/repeated body (§4-allowed), like a `for`
                    // body — not scanned.
                    top.check_item(index, cond, &mut self.former);
                }
                ast::StmtKind::Export(_) => unreachable!("top_inner unwraps Export"),
            }
        }
        self.top_level = Some(top);
    }

    /// Function declarations hoist with their signature types so
    /// calls anywhere in the scope type against them (SPEC §7).
    pub(super) fn hoist_functions(&mut self, items: &'a [ast::Stmt]) {
        for stmt in items {
            let decl = match &stmt.kind {
                ast::StmtKind::Function(d) => d,
                ast::StmtKind::Export(inner) => match &inner.kind {
                    ast::StmtKind::Function(d) => d,
                    _ => continue,
                },
                _ => continue,
            };
            let mut env = self.tyenv();
            for (i, p) in decl.type_params.iter().enumerate() {
                env.insert(self.former.text(p.span), Type::Var(i as u32));
            }
            let mut params = Vec::new();
            let mut variadic = None;
            let mut required = 0usize;
            for param in &decl.params {
                let ty = self.former.form_signature(&param.ty, &env);
                if param.variadic {
                    variadic = Some(Box::new(ty));
                } else {
                    if param.default.is_none() {
                        required += 1;
                    }
                    params.push(ty);
                }
            }
            let ret = match &decl.return_type {
                Some(r) => self.former.form_signature(r, &env),
                // Omitted returns infer when the body checks; until
                // then the function is pending and recursive calls
                // into it taint their caller (CDR-004 §7).
                None => {
                    let name = self.former.text(decl.name.span).to_string();
                    self.scopes
                        .last_mut()
                        .expect("pending stack non-empty")
                        .pending_returns
                        .insert(name);
                    Type::Unknown
                }
            };
            let name = self.former.text(decl.name.span).to_string();
            let param_names: Vec<String> = decl
                .params
                .iter()
                .filter(|p| !p.variadic)
                .map(|p| self.former.text(p.name.span).to_string())
                .collect();
            let param_defaulted: Vec<bool> = decl
                .params
                .iter()
                .filter(|p| !p.variadic)
                .map(|p| p.default.is_some())
                .collect();
            self.record_fn_meta(
                name.clone(),
                FnMeta {
                    vars: decl.type_params.len() as u32,
                    bounds: self.type_param_bound_names(decl),
                    required,
                    names: Some(param_names),
                    defaulted: param_defaulted,
                },
            );
            self.bind_decl(
                name,
                Type::Func {
                    params,
                    variadic,
                    ret: Box::new(ret),
                },
                false,
                decl.name.span,
            );
            if let Some(ty) = self.lookup(self.former.text(decl.name.span)).cloned() {
                // The scope binding keeps rank-1 `Var(i)` slots for call-site
                // inference.  The semantic declaration fact must not expose
                // those allocator-local slots, so project them to stable rigid
                // variables rooted at the written type-parameter spans.
                let origins = self.stable_type_parameter_substitutions(&decl.type_params);
                let semantic_ty = substitute(&ty, &origins);
                self.record_typed_node(
                    topaz_hir::TypedNodeKind::Declaration,
                    decl.name.span,
                    &semantic_ty,
                );
            }
        }
    }

    pub(super) fn check_stmt(&mut self, stmt: &'a ast::Stmt) {
        match &stmt.kind {
            ast::StmtKind::Import(_) => {}
            // `break (label)? (value)?` resolves the target loop, then
            // contribute the value's type (or Unit) to that loop's break-join.
            ast::StmtKind::Break { label, value } => self.check_break(label, value, stmt.span),
            // `continue (label)?` resolves the target loop (value-less).
            ast::StmtKind::Continue { label } => self.check_continue(label, stmt.span),
            ast::StmtKind::Export(inner) => {
                // `collect_methods` already reports `export impl` and deliberately
                // does not register its methods. Avoid checking malformed method
                // bodies as though the block were a valid local impl.
                if !matches!(&inner.kind, ast::StmtKind::Impl(_)) {
                    if let ast::StmtKind::Function(decl) = &inner.kind {
                        for bound in decl.type_param_bounds.iter().flatten() {
                            let protocol = self.former.text(bound.span);
                            if !matches!(protocol, "Eq" | "Order" | "Show" | "JSON") {
                                self.former.error(
                                    codes::NON_EXPORTABLE_BOUND,
                                    format!(
                                        "exported functions cannot expose module-local protocol bound `{protocol}`"
                                    ),
                                    bound.span,
                                );
                            }
                        }
                    }
                    self.check_stmt(inner);
                }
            }
            // Alias bodies are validated by frame collection.
            ast::StmtKind::TypeAlias(_) => {}
            // §3 enum decls are module-top-level nominal declarations. The parser
            // keeps `enum` contextual in every statement position for recovery,
            // but formation deliberately registers only top-level declarations;
            // reject a nested head explicitly instead of accepting an unusable
            // declaration whose constructor namespace can never resolve.
            ast::StmtKind::Enum(decl) => {
                if self.scopes.len() != 1 {
                    self.former.error(
                        codes::MALFORMED_TYPE,
                        "enum declarations are module-top-level only".to_string(),
                        decl.name.span,
                    );
                } else {
                    self.check_enum_decl(decl);
                }
            }
            // §3 record decls are module-top-level nominal declarations. As with
            // enums, formation deliberately registers only top-level heads. Reject
            // a nested shell explicitly instead of checking defaults on a record
            // whose type and constructor can never resolve.
            ast::StmtKind::Record(decl) => {
                if self.scopes.len() != 1 {
                    self.former.error(
                        codes::MALFORMED_TYPE,
                        "record declarations are module-top-level only".to_string(),
                        decl.name.span,
                    );
                } else {
                    self.check_record_decl(decl);
                }
            }
            // §3 newtype decls are module-top-level nominal declarations. Formation
            // deliberately collects only top-level heads; reject a nested dead shell
            // explicitly, matching the enum/record boundary, instead of accepting a
            // declaration whose type and constructor can never resolve.
            ast::StmtKind::Newtype(decl) => {
                if self.scopes.len() != 1 {
                    self.former.error(
                        codes::MALFORMED_TYPE,
                        "newtype declarations are module-top-level only".to_string(),
                        decl.name.span,
                    );
                } else {
                    self.check_newtype_decl(decl);
                }
            }
            // §4 (v5.4) impl blocks: methods are registered (signatures + coherence
            // + duplicate/collision diagnostics) at formation (`collect_methods`);
            // this pass type-checks each method BODY with `self` bound to the
            // receiver type.
            ast::StmtKind::Impl(decl) => {
                if self.scopes.len() != 1 {
                    self.former.error(
                        codes::MALFORMED_TYPE,
                        "impl declarations are module-top-level only".to_string(),
                        decl.name.span,
                    );
                } else {
                    self.check_impl(decl);
                }
            }
            // Protocols are module-top-level declarations. Formation only
            // collects that surface; reject a nested shell explicitly instead of
            // accepting an unusable declaration whose static head never registers.
            ast::StmtKind::Protocol(decl) => {
                if self.scopes.len() != 1 {
                    self.former.error(
                        codes::MALFORMED_TYPE,
                        "protocol declarations are module-top-level only".to_string(),
                        decl.name.span,
                    );
                }
            }
            ast::StmtKind::Function(decl) => self.check_function(decl),
            ast::StmtKind::Let {
                mutable,
                pattern,
                ty,
                value,
            } => self.check_let(*mutable, pattern, ty.as_ref(), value),
            ast::StmtKind::Using { name, value, body } => {
                let value_ty = self.infer(value);
                self.expect(&value_ty, &Type::File, value.span);
                self.push_scope();
                let text = self.former.text(name.span).to_string();
                self.record_typed_local(&text, name.span, &Type::File);
                self.bind_decl(text, Type::File, false, name.span);
                self.check_block(body);
                self.pop_scope();
            }
            ast::StmtKind::Const { name, ty, value } => {
                let bound = if let Some(annot) = ty {
                    let env = self.tyenv();
                    let annot_ty = self.former.form(annot, &env);
                    self.check_expr(value, &annot_ty);
                    annot_ty
                } else {
                    // `const` preserves literal types (CDR-004 §4).
                    self.infer(value)
                };
                // §2/§13a: a constant arithmetic fault (div-by-zero, overflow,
                // negative/overflowing exponent) is a STATIC error, not a runtime
                // fault — fold the value and report it like `run`/`build` do.
                self.const_fold(value);
                let text = self.former.text(name.span).to_string();
                self.propagate_fn_meta(value, &text);
                self.record_typed_local(&text, name.span, &bound);
                self.bind_decl(text, bound, false, name.span);
            }
            ast::StmtKind::Assign { target, value, op } => {
                // §4: an assignment target cannot route through
                // optional access — `?.` is conditional, not
                // assignable.
                if target_has_optional(target) {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        "cannot assign through optional access `?.` (§4)".to_string(),
                        target.span,
                    );
                }
                // §4/§9: an in-place assignment — record member OR
                // collection index — requires a mutable root binding.
                if let Some(root) = assignment_root(target) {
                    let name = self.former.text(root.span);
                    if self.mutability(name) == Some(false) {
                        self.former.error(
                            codes::IMMUTABLE,
                            format!("`{name}` is immutable; declare it with `let mut`"),
                            root.span,
                        );
                    }
                }
                // §9/§22: Map slots are not index-assignable — the
                // write anchors at the chain's LAST index segment,
                // wherever it sits in the path.
                if let Some(ast::ExprKind::Index { object, .. }) =
                    last_index_segment(target).map(|e| &e.kind)
                {
                    let object_ty = self.infer(object);
                    if matches!(object_ty, Type::Ctor(Ctor::Map, _)) {
                        self.former.error(
                            codes::TYPE_MISMATCH,
                            "a Map is not index-assignable; use `m.insert` (§22)".to_string(),
                            target.span,
                        );
                    }
                }
                let target_ty = self.infer(target);
                // Mutability tracking is a later phase; `=` checks
                // assignability, `??=` checks the §12 rule: the
                // target must be Option or nullable, and the value
                // must be assignable to the FULL target type — no
                // implicit `Some(value)` wrapping. Arithmetic
                // compounds stay staged.
                match op {
                    ast::AssignOp::Assign => {
                        self.check_expr(value, &target_ty);
                    }
                    ast::AssignOp::Coalesce => match unwrap_optional(&target_ty) {
                        Some(_) => {
                            self.check_expr(value, &target_ty);
                        }
                        None if !target_ty.has_unknown() => {
                            let display = target_ty.clone();
                            self.former.error(
                                codes::TYPE_MISMATCH,
                                format!(
                                    "`??=` needs an Option or nullable target, found `{display}`"
                                ),
                                target.span,
                            );
                            self.infer(value);
                        }
                        None => {
                            self.infer(value);
                        }
                    },
                    _ => {
                        // Arithmetic compounds: the operation must
                        // admit the operands and produce a value
                        // assignable back to the target.
                        let bop = match op {
                            ast::AssignOp::Add => ast::BinaryOp::Add,
                            ast::AssignOp::Sub => ast::BinaryOp::Sub,
                            ast::AssignOp::Mul => ast::BinaryOp::Mul,
                            ast::AssignOp::Div => ast::BinaryOp::Div,
                            ast::AssignOp::Rem => ast::BinaryOp::Rem,
                            _ => unreachable!("plain/coalesce handled above"),
                        };
                        let value_ty = self.infer(value).widen();
                        if !target_ty.has_unknown() && !value_ty.has_unknown() {
                            let result = self.binary_type(
                                bop,
                                target_ty.clone().widen(),
                                value_ty,
                                stmt.span,
                            );
                            if !result.has_unknown() {
                                self.expect(&result, &target_ty.clone().widen(), stmt.span);
                            }
                        }
                    }
                }
            }
            ast::StmtKind::Return(value) => {
                // §5/§7: `return` belongs to the innermost function or lambda body.
                // At the module top level there is no enclosing function, so the
                // interpreter runtime-faults it; reject it statically with the same
                // code/message (TPZ5001) so `check` gates exactly what `run` does.
                if self.ret_ctx.is_empty() {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        "`return` outside a function".to_string(),
                        stmt.span,
                    );
                }
                let expected = self.ret_ctx.last().cloned().flatten();
                match (value, expected) {
                    (Some(v), Some(ret)) => {
                        self.check_expr(v, &ret);
                    }
                    (Some(v), None) => {
                        let ty = self.infer(v);
                        if let Some(join) = self.ret_join.last_mut() {
                            join.push(ty);
                        }
                    }
                    (None, Some(ret)) => {
                        // `return` with no value yields `()`.
                        self.expect(&Type::Prim(Prim::Unit), &ret, stmt.span);
                    }
                    (None, None) => {
                        if let Some(join) = self.ret_join.last_mut() {
                            join.push(Type::Prim(Prim::Unit));
                        }
                    }
                }
            }
            ast::StmtKind::Defer(expr) => {
                self.infer(expr);
            }
            ast::StmtKind::Expr(expr) => match &expr.kind {
                ast::ExprKind::For {
                    pattern,
                    iter,
                    body,
                } => {
                    self.check_for(pattern, iter, body, true);
                }
                _ => {
                    self.infer(expr);
                }
            },
            ast::StmtKind::While { cond, body } => {
                let cond_ty = self.infer(cond);
                self.expect_bool(&cond_ty, cond.span);
                // A `while` is a valueless loop frame (mirrors `LoopBody`).
                self.loop_ctx.push(LoopFrame {
                    label: None,
                    value_loop: false,
                    bare_target: true,
                    bare_error: None,
                    expected: None,
                    breaks: Vec::new(),
                });
                self.check_block(body);
                self.loop_ctx.pop();
            }
        }
    }

    pub(super) fn check_let(
        &mut self,
        mutable: bool,
        pattern: &'a ast::Pattern,
        annot: Option<&'a ast::Type>,
        value: &'a ast::Expr,
    ) {
        // `let name: T = …` parses the annotation as a Typed PATTERN;
        // the statement-level `ty` slot covers the remaining forms.
        let annotation = annot.or(match &pattern.kind {
            ast::PatternKind::Typed { ty, .. } => Some(ty),
            _ => None,
        });
        let bound = if let Some(annot) = annotation {
            let env = self.tyenv();
            let annot_ty = self.former.form(annot, &env);
            self.check_expr(value, &annot_ty);
            annot_ty
        } else {
            // Unannotated `let` and every `let mut` widen literals
            // (CDR-004 §4).
            self.at_bare_binding = true;
            let t = self.infer(value).widen();
            self.at_bare_binding = false; // already consumed; belt and braces
            t
        };
        self.record_typed_node(topaz_hir::TypedNodeKind::Pattern, pattern.span, &bound);
        match &pattern.kind {
            ast::PatternKind::Binding(name) | ast::PatternKind::Typed { name, .. } => {
                let text = self.former.text(name.span).to_string();
                self.propagate_fn_meta(value, &text);
                self.record_typed_local(&text, name.span, &bound);
                self.bind_decl(text, bound, mutable, name.span);
            }
            _ => {
                // A `let` destructuring pattern is a BINDING context (not a match
                // arm), so a bare name binds rather than gating as a variant typo.
                let cov = self.bind_match_pattern_at(pattern, &bound, false);
                // §4 (v5.4) REFUTABILITY: a `let` binds UNCONDITIONALLY, so its
                // pattern must be IRREFUTABLE — it must cover every value of the
                // scrutinee type. A refutable pattern (an enum variant when the enum
                // has >1 variant, a literal, a range, or a record/nominal pattern
                // with a refutable field) would pass `check` then FAULT at runtime
                // on a non-matching value; reject it here so check==runtime. The
                // irrefutability test reuses the match exhaustiveness `covers` logic
                // (a pattern is irrefutable iff it alone exhausts its scrutinee type).
                // `if let` is the refutable counterpart (it has a non-matching arm).
                //
                // LIST DESTRUCTURING is version-gated: the frozen v5.1/v5.2/v5.3
                // corpus keeps its historical behavior, but at v5.4 a list `let`
                // requiring any concrete element position (`[a, b]`, `[a, ..rest]`,
                // nested fixed-rest forms, etc.) is length-refutable and must use
                // `if let`. A pure rest pattern (`[..rest]` / `[..]`) remains
                // irrefutable for an Array because it matches every length.
                let pre_v54_list_exempt = self.former.version() < LangVersion::V5_4
                    && matches!(pattern.kind, ast::PatternKind::List(_));
                let list_refutable = self.former.version() >= LangVersion::V5_4
                    && list_let_pattern_refutable(pattern);
                let refutable = !pre_v54_list_exempt
                    && !bound.has_unknown()
                    && (list_refutable || !cov.covers(&bound, self.former.enum_table()));
                if refutable {
                    self.former.error(
                        codes::REFUTABLE_LET,
                        "refutable pattern in `let`: it does not match every value of \
                         the type — use `if let` to handle the non-matching case"
                            .to_string(),
                        pattern.span,
                    );
                }
            }
        }
    }
}
