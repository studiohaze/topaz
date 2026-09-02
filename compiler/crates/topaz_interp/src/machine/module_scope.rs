use super::*;

impl<'a> Machine<'a> {
    pub fn new(src: &str, host: &'a dyn Host) -> Self {
        Self::new_with_version(src, host, LangVersion::CURRENT)
    }

    pub fn new_with_version(src: &str, host: &'a dyn Host, language_version: LangVersion) -> Self {
        let globals = Rc::new(RefCell::new(Scope {
            vars: HashMap::new(),
            parent: None,
            defers: Vec::new(),
            aliases: BTreeMap::new(),
        }));
        Machine {
            language_version,
            src: src.into(),
            host,
            frames: Vec::new(),
            values: Vec::new(),
            env: globals,
            module_scopes: BTreeMap::new(),
            current_module: CurrentModuleContext::default(),
            source_module_index: BTreeMap::new(),
            enum_defs: BTreeMap::new(),
            record_defs: BTreeMap::new(),
            newtype_defs: NewtypeTable::new(),
            method_defs: BTreeMap::new(),
            protocol_defs: {
                // The builtin protocols are always available (their derived
                // conformances dispatch to the value leaves); user protocols add to
                // this set at load.
                let mut s = std::collections::BTreeSet::new();
                s.insert("Show".to_string());
                s.insert("Eq".to_string());
                s.insert("Order".to_string());
                s
            },
            type_params: Rc::from([] as [Ident; 0]),
            call_depth: 0,
            record_default_depth: 0,
            comp_accs: Vec::new(),
        }
    }

    pub(super) fn contained_execution(&self, frames: Vec<Frame>) -> ExecutionContext {
        ExecutionContext {
            frames,
            values: Vec::new(),
            env: self.env.clone(),
            src: self.src.clone(),
            type_params: self.type_params.clone(),
            call_depth: self.call_depth,
            record_default_depth: self.record_default_depth,
            comp_accs: Vec::new(),
        }
    }

    /// Swap every mutable execution field as one ownership boundary. Suspended
    /// arm and deferred sub-run code must use this rather than naming fields.
    pub(super) fn swap_execution(&mut self, execution: &mut ExecutionContext) {
        std::mem::swap(&mut self.frames, &mut execution.frames);
        std::mem::swap(&mut self.values, &mut execution.values);
        std::mem::swap(&mut self.env, &mut execution.env);
        std::mem::swap(&mut self.src, &mut execution.src);
        std::mem::swap(&mut self.type_params, &mut execution.type_params);
        std::mem::swap(&mut self.call_depth, &mut execution.call_depth);
        std::mem::swap(
            &mut self.record_default_depth,
            &mut execution.record_default_depth,
        );
        std::mem::swap(&mut self.comp_accs, &mut execution.comp_accs);
    }

    pub(super) fn text(&self, span: Span) -> &str {
        &self.src[span.lo as usize..span.hi as usize]
    }

    pub(super) fn bind(&mut self, name: String, value: Value, mutable: bool) {
        self.env
            .borrow_mut()
            .vars
            .insert(name, BindingCell { value, mutable });
    }

    /// §4: same-scope redeclaration is a static error — dynamic
    /// guard here. Fresh scopes (calls, blocks, loop iterations) are
    /// new maps, so cross-scope shadowing stays legal.
    pub(super) fn bind_checked(
        &mut self,
        name: String,
        value: Value,
        mutable: bool,
        span: Span,
    ) -> Result<(), RtError> {
        if self.env.borrow().vars.contains_key(&name) {
            return Err(fault(
                codes::GUARD_REDECLARE,
                format!("`{name}` is already declared in this scope (§4)"),
                span,
            ));
        }
        self.bind(name, value, mutable);
        Ok(())
    }

    pub(super) fn plan_module_callables(&self, program: &Program) -> ModuleCallablePlan {
        let mut plan = ModuleCallablePlan::default();
        for (index, stmt) in program.items.iter().enumerate() {
            let inner = match &stmt.kind {
                StmtKind::Export(inner) => &**inner,
                _ => stmt,
            };
            match &inner.kind {
                StmtKind::Impl(decl) => {
                    plan.item_indices.push(index);
                    if decl.target.is_none() {
                        plan.inherent_method_targets
                            .insert(self.text(decl.name.span).to_string());
                    }
                }
                StmtKind::Protocol(_) => plan.item_indices.push(index),
                _ => {}
            }
        }
        plan
    }

    pub(super) fn render_import_path(&self, import: &ImportItem) -> String {
        let mut path = String::new();
        for (index, segment) in import.path.segments.iter().enumerate() {
            if index > 0 {
                path.push('.');
            }
            path.push_str(self.text(segment.span));
        }
        path
    }

    pub(super) fn register_module_callables(
        &mut self,
        program: &Program,
        declaration_identity: &str,
        plan: &ModuleCallablePlan,
    ) {
        for &index in &plan.item_indices {
            let stmt = &program.items[index];
            let inner = match &stmt.kind {
                StmtKind::Export(inner) => &**inner,
                _ => stmt,
            };
            match &inner.kind {
                StmtKind::Impl(decl) => {
                    // Manual protocol implementations use a protocol-qualified
                    // key; inherent implementations use the nominal method identity.
                    let key_id = if let Some(target) = decl.target {
                        let protocol = self.text(decl.name.span).to_string();
                        let type_id = self.text(target.span).to_string();
                        protocol_method_identity(declaration_identity, &protocol, &type_id)
                    } else {
                        let type_id = self.text(decl.name.span).to_string();
                        self.method_identity_for_lookup(&type_id)
                            .map(|identity| identity.to_string())
                            .unwrap_or(type_id)
                    };
                    for method in &decl.methods {
                        let name = self.text(method.decl.name.span).to_string();
                        let closure = Value::Closure(Rc::new(ClosureData {
                            name: Some(name.clone()),
                            params: ClosureParams::Declared(Rc::from(
                                method.decl.params.as_slice(),
                            )),
                            body: ClosureBody::Block(method.decl.body.clone()),
                            env: self.env.clone(),
                            src: self.src.clone(),
                            type_params: Rc::from(method.decl.type_params.as_slice()),
                            return_type: method.decl.return_type.clone(),
                        }));
                        self.method_defs.insert((key_id.clone(), name), closure);
                    }
                }
                StmtKind::Protocol(decl) => {
                    self.protocol_defs
                        .insert(self.text(decl.name.span).to_string());
                }
                _ => unreachable!("module callable plan contains a non-callable item"),
            }
        }
    }

    pub(super) fn collect_module_metadata(
        &self,
        program: &Program,
        declaration_identity: &str,
        inherent_method_targets: &std::collections::BTreeSet<String>,
        module_types: &mut ModuleTypeScope,
    ) -> std::collections::BTreeSet<String> {
        let mut private_runtime_values = std::collections::BTreeSet::new();
        for stmt in &program.items {
            let (inner, exported) = match &stmt.kind {
                StmtKind::Export(inner) => (&**inner, true),
                _ => (stmt, false),
            };
            if !exported
                && let StmtKind::Let {
                    mutable: false,
                    pattern,
                    ..
                } = &inner.kind
                && let PatternKind::Binding(name) | PatternKind::Typed { name, .. } = &pattern.kind
            {
                private_runtime_values.insert(self.text(name.span).to_string());
            }
            if let StmtKind::Import(import) = &stmt.kind {
                let target = self.render_import_path(import);
                let target = self
                    .module_scopes
                    .get_key_value(target.as_str())
                    .map(|(identity, _)| identity.clone())
                    .unwrap_or_else(|| Rc::from(target));
                match &import.kind {
                    ImportKind::Namespace { alias } => {
                        let local = alias
                            .as_ref()
                            .map(|alias| self.text(alias.span).to_string())
                            .unwrap_or_else(|| {
                                self.text(
                                    import
                                        .path
                                        .segments
                                        .last()
                                        .expect("nonempty import path")
                                        .span,
                                )
                                .to_string()
                            });
                        module_types.schema_imports.namespaces.insert(local, target);
                    }
                    ImportKind::Selected { specs } => {
                        for spec in specs {
                            let imported = self.text(spec.name.span).to_string();
                            let local = spec
                                .alias
                                .as_ref()
                                .map(|alias| self.text(alias.span).to_string())
                                .unwrap_or_else(|| imported.clone());
                            module_types
                                .schema_imports
                                .selected
                                .insert(local, (target.clone(), imported));
                        }
                    }
                }
            }
            match &inner.kind {
                StmtKind::TypeAlias(alias) => {
                    let name = self.text(alias.name.span);
                    module_types.aliases.insert(
                        name.to_string(),
                        (
                            alias.type_params.as_slice().into(),
                            alias.ty.clone(),
                            exported,
                        ),
                    );
                    module_types
                        .schema_decls
                        .aliases
                        .insert(name.to_string(), alias.clone());
                }
                StmtKind::Enum(decl) => {
                    let name = self.text(decl.name.span).to_string();
                    let schema_name = name.clone();
                    // Reserved prelude constructors retain their builtin meaning
                    // even under unchecked execution. Declaration indices remain
                    // positions in the full source declaration (run≡build).
                    let variants: Rc<EnumVariants> = Rc::new(
                        decl.variants
                            .iter()
                            .enumerate()
                            .map(|(i, variant)| {
                                let name = self.text(variant.name.span).to_string();
                                let arity = variant.payload.as_ref().map_or(0, |types| types.len());
                                (name, (arity, i as u32))
                            })
                            .filter(|(name, _)| !is_reserved_variant_name(name))
                            .collect(),
                    );
                    let method_identity = (self.language_version >= LangVersion::V5_20
                        || inherent_method_targets.contains(&name))
                    .then(|| Rc::from(receiver_method_identity(declaration_identity, &name)));
                    let definition = EnumRuntimeDef {
                        runtime_id: Rc::from(schema_name.as_str()),
                        method_identity,
                        variants,
                        decl: decl.clone(),
                        decl_src: self.src.clone(),
                    };
                    module_types
                        .nominals
                        .enum_defs
                        .insert(schema_name.clone(), definition);
                    module_types
                        .schema_decls
                        .enums
                        .insert(schema_name, decl.clone());
                }
                StmtKind::Record(decl) => {
                    let name = self.text(decl.name.span).to_string();
                    let schema_name = name.clone();
                    let fields: RecordFields = decl
                        .fields
                        .iter()
                        .map(|field| {
                            let name = self.text(field.name.span).to_string();
                            let default = field.default.as_ref().map(|expr| RecordDefault {
                                src: self.src.clone(),
                                env: self.env.clone(),
                                expr: expr.clone(),
                            });
                            (name, default)
                        })
                        .collect::<Vec<_>>()
                        .into();
                    let method_identity = (self.language_version >= LangVersion::V5_20
                        || inherent_method_targets.contains(&name))
                    .then(|| Rc::from(receiver_method_identity(declaration_identity, &name)));
                    let definition = RecordRuntimeDef {
                        runtime_id: Rc::from(schema_name.as_str()),
                        method_identity,
                        fields,
                        decl: decl.clone(),
                        decl_src: self.src.clone(),
                    };
                    module_types
                        .nominals
                        .record_defs
                        .insert(schema_name.clone(), definition);
                    module_types
                        .schema_decls
                        .records
                        .insert(schema_name, decl.clone());
                }
                StmtKind::Newtype(decl) => {
                    let name = self.text(decl.name.span).to_string();
                    let schema_name = name.clone();
                    let method_identity = (self.language_version >= LangVersion::V5_20
                        || inherent_method_targets.contains(&name))
                    .then(|| Rc::from(receiver_method_identity(declaration_identity, &name)));
                    let definition = NewtypeRuntimeDef {
                        runtime_id: Rc::from(schema_name.as_str()),
                        method_identity,
                        decl: decl.clone(),
                        decl_src: self.src.clone(),
                    };
                    module_types
                        .nominals
                        .newtype_defs
                        .insert(schema_name.clone(), definition);
                    module_types
                        .schema_decls
                        .newtypes
                        .insert(schema_name, decl.clone());
                }
                _ => {}
            }
        }
        private_runtime_values
    }

    pub(super) fn build_module_admission(
        &self,
        program: &Program,
        identity: ModuleAdmissionIdentity,
        aliases: AliasTable,
    ) -> ModuleAdmission {
        let callables = self.plan_module_callables(program);
        let mut types = ModuleTypeScope {
            declaration_identity: identity.declaration.clone(),
            src: self.src.clone(),
            aliases,
            schema_decls: SchemaDeclTables::default(),
            schema_imports: SchemaImportScope::default(),
            nominals: ModuleNominalDefs::default(),
        };
        let private_default_values = self.collect_module_metadata(
            program,
            &identity.declaration,
            &callables.inherent_method_targets,
            &mut types,
        );
        ModuleAdmission {
            identity,
            types,
            private_default_values,
            callables,
        }
    }

    pub(super) fn install_global_nominals(&mut self, nominals: &ModuleNominalDefs) {
        self.enum_defs.extend(
            nominals
                .enum_defs
                .iter()
                .map(|(name, definition)| (name.clone(), definition.clone())),
        );
        self.record_defs.extend(
            nominals
                .record_defs
                .iter()
                .map(|(name, definition)| (name.clone(), definition.clone())),
        );
        self.newtype_defs.extend(
            nominals
                .newtype_defs
                .iter()
                .map(|(name, definition)| (name.clone(), definition.clone())),
        );
    }

    pub(super) fn install_module_admission(
        &mut self,
        program: &Program,
        admission: ModuleAdmission,
    ) {
        let ModuleAdmission {
            identity,
            types,
            private_default_values,
            callables,
        } = admission;
        self.install_global_nominals(&types.nominals);
        let env = self.env.clone();
        match self.module_scopes.entry(identity.runtime_scope) {
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let module_scope = entry.get_mut();
                module_scope.runtime.private_default_values = private_default_values;
                module_scope.types = types;
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                let mut runtime = ModuleRuntimeScope::new(env);
                runtime.private_default_values = private_default_values;
                entry.insert(ModuleScope { runtime, types });
            }
        }
        self.register_module_callables(program, &identity.declaration, &callables);
    }

    /// Run a program's items top to bottom; the value of the final
    /// expression statement (if any) is returned for tests, `Unit`
    /// otherwise.
    pub fn run_program(&mut self, program: &Program) -> RunResult {
        // §6 runtime conformance reads the module's top-level alias
        // table.
        let identity = ModuleAdmissionIdentity {
            declaration: self.current_module.identity.declaration.clone(),
            runtime_scope: self.current_module.identity.runtime_scope.clone(),
        };
        let source_key = self.src.as_ptr() as usize;
        let aliases = self
            .module_scopes
            .get_mut(&identity.runtime_scope)
            .map(|scope| std::mem::take(&mut scope.types.aliases))
            .unwrap_or_default();
        self.source_module_index
            .insert(source_key, identity.runtime_scope.clone());
        let admission = self.build_module_admission(program, identity, aliases);
        self.install_module_admission(program, admission);
        // §4/§17 + CDR-003 §5: top-level `const` items evaluate at
        // load time, before any runtime initialization, with no host
        // effects possible.
        self.const_pass(program)?;
        let mut last = Value::Unit;
        for stmt in &program.items {
            // Top-level expression statements keep their value so
            // tests and interactive consumers can observe it; all other
            // statements yield Unit.
            match &stmt.kind {
                StmtKind::Expr(e) => {
                    if let ExprKind::For {
                        pattern,
                        iter,
                        body,
                    } = &e.kind
                    {
                        // Top-level `for` is statement-position (§5).
                        self.frames.push(Frame::KForStart {
                            pattern: pattern.clone(),
                            body: body.clone(),
                            span: e.span,
                            is_stmt: true,
                        });
                        self.frames.push(Frame::Eval(iter.clone()));
                    } else {
                        self.eval_expr(e)?;
                    }
                }
                _ => self.exec_stmt(stmt)?,
            }
            last = self.run_to_completion()?;
        }
        // The entry's top-level scope exits at program end (§14).
        self.drain_defers();
        Ok(last)
    }

    pub(super) fn run_unit_initialized(
        unit: &topaz_resolve::ResolveOutput,
        host: &'a dyn Host,
    ) -> Result<(Self, Value), RtError> {
        let mut machine = Machine::new_with_version("", host, unit.language_version);
        let mut last = Value::Unit;
        for module in &unit.modules {
            let runtime_scope: Rc<str> = Rc::from(module.identity.as_str());
            machine.src = unit.map.file(module.file).src().into();
            machine.env = Rc::new(RefCell::new(Scope {
                vars: HashMap::new(),
                parent: None,
                defers: Vec::new(),
                aliases: BTreeMap::new(),
            }));
            machine.current_module = CurrentModuleContext {
                identity: CurrentModuleIdentity {
                    declaration: if module.is_entry {
                        Rc::default()
                    } else {
                        runtime_scope.clone()
                    },
                    runtime_scope,
                },
                is_extern: module.is_extern,
            };
            if module.is_generated_std {
                let intrinsics: &[(&str, Builtin)] = match module.identity.as_str() {
                    "std.lispex" => &[
                        (
                            "__lispexValueFromCanonical",
                            Builtin::LispexValueFromCanonical,
                        ),
                        ("__lispexCanonicalBytes", Builtin::LispexCanonicalBytes),
                        ("__lispexDefaultLimits", Builtin::LispexDefaultLimits),
                        ("__lispexInspectRule", Builtin::LispexInspectRule),
                        ("__lispexEvaluate", Builtin::LispexEvaluate),
                        (
                            "__lispexEvaluateWithEvidence",
                            Builtin::LispexEvaluateWithEvidence,
                        ),
                        (
                            "__lispexConsumerArtifactFromBytes",
                            Builtin::LispexConsumerArtifactFromBytes,
                        ),
                        (
                            "__lispexConsumerArtifactBytes",
                            Builtin::LispexConsumerArtifactBytes,
                        ),
                        (
                            "__lispexPortableCoreBytes",
                            Builtin::LispexPortableCoreBytes,
                        ),
                        (
                            "__lispexInspectConsumerArtifact",
                            Builtin::LispexInspectConsumerArtifact,
                        ),
                        (
                            "__lispexVerifyConsumerArtifact",
                            Builtin::LispexVerifyConsumerArtifact,
                        ),
                        ("__lispexFreshReplay", Builtin::LispexFreshReplay),
                    ],
                    "std.lispex.rules" => &[("__lispexRule", Builtin::LispexRule)],
                    _ => &[],
                };
                for (name, kind) in intrinsics {
                    machine.bind(
                        (*name).to_string(),
                        Value::Builtin {
                            kind: *kind,
                            recv: None,
                        },
                        false,
                    );
                }
            }
            last = machine.run_program(&module.program).map_err(|e| RtError {
                code: e.code,
                message: if module.is_entry {
                    e.message
                } else {
                    format!(
                        "{} (during initialization of module `{}`; {})",
                        e.message,
                        module.identity,
                        topaz_resolve::import_chain(unit, &module.identity)
                    )
                },
                span: e.span,
            })?;
        }
        Ok((machine, last))
    }

    /// §17 unit execution (CDR-003 §9): modules initialize in the
    /// resolver's normative order (the `modules` list is already
    /// sorted; the entry is last), each in its own top-level
    /// environment; the entry then runs with full v5.1-compatible
    /// entry semantics. An init fault aborts with module context.
    pub fn run_unit(unit: &topaz_resolve::ResolveOutput, host: &'a dyn Host) -> RunResult {
        let (_, last) = Self::run_unit_initialized(unit, host)?;
        Ok(last)
    }

    /// v5.4 explicit CLI entrypoint execution: run the same module initialization
    /// as [`Self::run_unit`], then, only when the entry exports `main`, call that function
    /// with explicit `args` and `stdin` values.
    pub fn run_unit_with_main(
        unit: &topaz_resolve::ResolveOutput,
        host: &'a dyn Host,
        args: &[String],
        stdin: &str,
    ) -> RunResult {
        let (mut machine, last) = Self::run_unit_initialized(unit, host)?;
        let Some(main_span) = topaz_resolve::explicit_main_span(unit) else {
            return Ok(last);
        };
        let main = {
            let env = machine.env.borrow();
            env.vars.get("main").map(|cell| cell.value.clone())
        }
        .ok_or_else(|| {
            fault(
                codes::GUARD_UNBOUND,
                "`main` is not bound after entry initialization",
                main_span,
            )
        })?;
        let argv = Value::array(args.iter().map(Value::str).collect());
        machine.values.push(main);
        machine.values.push(argv);
        machine.values.push(Value::str(stdin));
        machine.apply_call(2, Vec::new(), Vec::new(), false, main_span)?;
        machine.run_to_completion()
    }

    /// Invoke one explicitly exported value from the entry module after normal
    /// module initialization. This is a library boundary for trusted product
    /// adapters that have already resolved and checked the unit; it does not
    /// add a Topaz source form or expose module-private bindings.
    pub fn run_unit_export(
        unit: &topaz_resolve::ResolveOutput,
        host: &'a dyn Host,
        name: &str,
        args: Vec<Value>,
    ) -> RunResult {
        let entry_file = unit
            .modules
            .iter()
            .find(|module| module.is_entry)
            .map(|module| module.file)
            .ok_or_else(|| {
                fault(
                    codes::GUARD_UNBOUND,
                    "entry module is missing",
                    Span::new(topaz_diag::FileId(0), 0, 0),
                )
            })?;
        let export_span = unit
            .name_facts
            .exports
            .iter()
            .find(|export| {
                export.file == entry_file
                    && export.name == name
                    && export.namespace == topaz_resolve::ResolvedNamespace::Value
            })
            .map(|export| export.declaration_span)
            .ok_or_else(|| {
                fault(
                    codes::GUARD_UNBOUND,
                    format!("`{name}` is not exported by the entry module"),
                    Span::new(entry_file, 0, 0),
                )
            })?;
        let (mut machine, _) = Self::run_unit_initialized(unit, host)?;
        let callable = {
            let env = machine.env.borrow();
            env.vars.get(name).map(|cell| cell.value.clone())
        }
        .ok_or_else(|| {
            fault(
                codes::GUARD_UNBOUND,
                format!("exported `{name}` is not bound after entry initialization"),
                export_span,
            )
        })?;
        let argc = args.len();
        machine.values.push(callable);
        machine.values.extend(args);
        machine.apply_call(argc, Vec::new(), Vec::new(), false, export_span)?;
        machine.run_to_completion()
    }

    /// Load-time const evaluation (§4: const expressions only; a
    /// non-const initializer is a dynamic guard, never host
    /// effects).
    pub(super) fn const_pass(&mut self, program: &Program) -> Result<(), RtError> {
        for stmt in &program.items {
            let inner = match &stmt.kind {
                StmtKind::Export(inner) => inner,
                _ => stmt,
            };
            if let StmtKind::Const { name, value, .. } = &inner.kind {
                let v = self.const_eval(value)?;
                self.bind_checked(self.text(name.span).to_string(), v, false, name.span)?;
            }
        }
        Ok(())
    }

    pub(super) fn const_eval(&self, expr: &Expr) -> Result<Value, RtError> {
        let non_const = || {
            fault(
                codes::GUARD_TYPE,
                "`const` initializers must be constant expressions (§4)",
                expr.span,
            )
        };
        match &expr.kind {
            ExprKind::Int
            | ExprKind::Float
            | ExprKind::Bool(_)
            | ExprKind::Null
            | ExprKind::Unit
            | ExprKind::String(_) => self.literal_value(expr).map_err(|_| non_const()),
            ExprKind::Paren(inner) => self.const_eval(inner),
            ExprKind::Ident => {
                // Earlier consts of the same module.
                let name = self.text(expr.span);
                lookup(&self.env, name).ok_or_else(non_const)
            }
            ExprKind::Unary { op, operand } => {
                let v = self.const_eval(operand)?;
                const_guarded(unary_value(*op, v, expr.span), expr.span)
            }
            ExprKind::Binary { op, lhs, rhs }
                if !matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Coalesce) =>
            {
                let l = self.const_eval(lhs)?;
                let r = self.const_eval(rhs)?;
                const_guarded(binary_value(*op, l, r, expr.span), expr.span)
            }
            _ => Err(non_const()),
        }
    }

    /// §17 import binding at module-init time. The resolver has
    /// already validated the unit, and the normative order
    /// guarantees the target module is initialized.
    pub(super) fn exec_import(&mut self, item: &ImportItem, span: Span) -> Result<(), RtError> {
        let target = self.render_import_path(item);
        if !self.module_scopes.contains_key(target.as_str()) {
            return Err(fault(
                codes::GUARD_UNIMPLEMENTED,
                format!(
                    "module `{target}` is not part of this run (use `topaz run` for multi-file units)"
                ),
                span,
            ));
        }
        match &item.kind {
            ImportKind::Namespace { alias } => {
                let bound = match alias {
                    Some(a) => self.text(a.span).to_string(),
                    None => self
                        .text(item.path.segments.last().expect("nonempty path").span)
                        .to_string(),
                };
                self.bind_checked(
                    bound,
                    Value::Namespace(Rc::from(target.as_str())),
                    false,
                    span,
                )
            }
            ImportKind::Selected { specs } => {
                for spec in specs {
                    let name = self.text(spec.name.span).to_string();
                    let projection = self.selected_import_projection(&target, &name, span)?;
                    let bound = match &spec.alias {
                        Some(a) => self.text(a.span).to_string(),
                        None => name.clone(),
                    };
                    let Some(value) = self.bind_selected_import(&bound, projection) else {
                        continue;
                    };
                    self.bind_checked(bound, value, false, span)?;
                }
                Ok(())
            }
        }
    }

    pub(super) fn selected_import_projection(
        &self,
        target: &str,
        imported: &str,
        span: Span,
    ) -> Result<SelectedImportProjection, RtError> {
        let module_scope = self.module_scopes.get(target).expect("initialized module");
        let value = self.exported_binding_in_scope(module_scope, target, imported, span)?;
        let module_nominals = &module_scope.types.nominals;
        Ok(SelectedImportProjection {
            value,
            enum_definition: module_nominals.enum_defs.get(imported).cloned(),
            record_definition: module_nominals.record_defs.get(imported).cloned(),
            newtype_definition: module_nominals.newtype_defs.get(imported).cloned(),
        })
    }

    pub(super) fn bind_selected_import(
        &mut self,
        bound: &str,
        projection: SelectedImportProjection,
    ) -> Option<Value> {
        if let Some(definition) = projection.enum_definition {
            self.enum_defs.insert(bound.to_string(), definition);
        }
        if let Some(definition) = projection.record_definition {
            self.record_defs.insert(bound.to_string(), definition);
        }
        if let Some(definition) = projection.newtype_definition {
            self.newtype_defs.insert(bound.to_string(), definition);
        }
        projection.value
    }

    pub(super) fn module_nominals_for_source(&self, src: &Rc<str>) -> Option<&ModuleNominalDefs> {
        self.source_module_index
            .get(&(src.as_ptr() as usize))
            .and_then(|module| self.module_scopes.get(module))
            .map(|scope| &scope.types.nominals)
    }

    pub(super) fn enum_definition_in(&self, src: &Rc<str>, name: &str) -> Option<&EnumRuntimeDef> {
        self.module_nominals_for_source(src)
            .and_then(|nominals| nominals.enum_defs.get(name))
            .or_else(|| self.enum_defs.get(name))
    }

    pub(super) fn record_definition_in(
        &self,
        src: &Rc<str>,
        name: &str,
    ) -> Option<&RecordRuntimeDef> {
        self.module_nominals_for_source(src)
            .and_then(|nominals| nominals.record_defs.get(name))
            .or_else(|| self.record_defs.get(name))
    }

    pub(super) fn newtype_definition_in(
        &self,
        src: &Rc<str>,
        name: &str,
    ) -> Option<&NewtypeRuntimeDef> {
        self.module_nominals_for_source(src)
            .and_then(|nominals| nominals.newtype_defs.get(name))
            .or_else(|| self.newtype_defs.get(name))
    }

    pub(super) fn method_identity_for_lookup(&self, lookup: &str) -> Option<Rc<str>> {
        self.enum_definition_in(&self.src, lookup)
            .and_then(|definition| definition.method_identity.clone())
            .or_else(|| {
                self.record_definition_in(&self.src, lookup)
                    .and_then(|definition| definition.method_identity.clone())
            })
            .or_else(|| {
                self.newtype_definition_in(&self.src, lookup)
                    .and_then(|definition| definition.method_identity.clone())
            })
    }

    pub(super) fn nominal_identity_projection(
        &self,
        method: Option<Rc<str>>,
    ) -> NominalIdentityProjection {
        let declaration = (self.language_version >= LangVersion::V5_20)
            .then(|| method.clone())
            .flatten();
        NominalIdentityProjection {
            declaration,
            method,
        }
    }

    pub(super) fn nominal_definition_identity<'b>(
        &self,
        method_identity: Option<&'b Rc<str>>,
        runtime_id: &'b Rc<str>,
    ) -> &'b str {
        if self.language_version >= LangVersion::V5_20 {
            method_identity.map_or(runtime_id.as_ref(), |identity| identity.as_ref())
        } else {
            runtime_id.as_ref()
        }
    }

    pub(super) fn enum_variants_for_value(&self, value: &Value) -> Option<&EnumVariants> {
        let Value::Enum { enum_id, .. } = value else {
            return None;
        };
        if self.language_version < LangVersion::V5_20 {
            return self
                .enum_defs
                .get(enum_id.as_ref())
                .map(|definition| definition.variants.as_ref());
        }
        let identity = value.nominal_declaration_id()?;
        self.module_scopes
            .values()
            .find_map(|scope| {
                scope
                    .types
                    .nominals
                    .enum_defs
                    .values()
                    .find_map(|definition| {
                        (definition
                            .method_identity
                            .as_deref()
                            .unwrap_or(definition.runtime_id.as_ref())
                            == identity)
                            .then_some(definition.variants.as_ref())
                    })
            })
            .or_else(|| {
                self.enum_defs.values().find_map(|definition| {
                    (definition
                        .method_identity
                        .as_deref()
                        .unwrap_or(definition.runtime_id.as_ref())
                        == identity)
                        .then_some(definition.variants.as_ref())
                })
            })
    }

    /// Admit an exported module name and return its runtime binding when present.
    /// Type-only exports have no runtime binding, so selected imports may consume
    /// `None` while runtime namespace access requires a value.
    pub(super) fn exported_binding_in_scope(
        &self,
        module_scope: &ModuleScope,
        module: &str,
        name: &str,
        span: Span,
    ) -> Result<Option<Value>, RtError> {
        let exported = module_scope.runtime.exports.contains(name);
        if !exported {
            return Err(fault(
                codes::GUARD_TYPE,
                format!("`{name}` is not exported by `{module}` (§17)"),
                span,
            ));
        }
        Ok(lookup(&module_scope.runtime.env, name))
    }

    /// Look up an exported runtime binding of an initialized module.
    pub(super) fn exported_value(
        &self,
        module_scope: &ModuleScope,
        module: &str,
        name: &str,
        span: Span,
    ) -> Result<Value, RtError> {
        self.exported_binding_in_scope(module_scope, module, name, span)?
            .ok_or_else(|| {
                fault(
                    codes::GUARD_TYPE,
                    format!("`{name}` is exported by `{module}` but has no runtime value (§17)"),
                    span,
                )
            })
    }
}
