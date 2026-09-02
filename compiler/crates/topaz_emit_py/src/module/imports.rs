use crate::*;

pub(super) fn emit_extern_imported_module<'a>(
    unit: &'a ResolveOutput,
    module: &'a topaz_resolve::ResolvedModule,
    checked_aliases: Option<&'a CheckedAliasSurfaces>,
    module_default_inputs: &ModuleDefaultInputFacts,
    module_definitions: &ModuleDefinitions<'a>,
    schema_modules: &Rc<JsonSchemaModules<'a>>,
    out: &mut String,
) -> Result<(ModuleRuntimeExports<'a>, String), PyEmitError> {
    let module_aliases = checked_aliases.and_then(|aliases| aliases.get(&module.identity));
    let mut ctx = Ctx::new(
        &unit.map,
        module_definitions.records.clone(),
        module_definitions.newtypes.clone(),
        module_definitions.enums.clone(),
        module_aliases,
    );
    ctx.module_identity = &module.identity;
    ctx.method_module_identity = Some(&module.identity);
    ctx.schema_modules = Rc::clone(schema_modules);
    register_module_functions(
        &module_definitions.functions,
        Some(&module.identity),
        &module_default_inputs.module_top_bound_names,
        &mut ctx,
    );
    enrich_module_function_mutation_metadata(&module_definitions.functions, &mut ctx);

    let mut exports = ModuleRuntimeExportMap::new();
    let mut init_body = String::new();
    for stmt in module_definitions.exported_statements.iter().copied() {
        let inner = exported_inner(stmt);
        let StmtKind::Function(decl) = &inner.kind else {
            return Err(PyEmitError::unsupported("extern module export").at(stmt.span));
        };
        let source_name = ctx.text(decl.name.span).to_string();
        let mut info = ctx
            .function_info(&source_name)
            .expect("registered extern module function")
            .clone();
        info.cooperative_py_name = None;
        let value_py = format!(
            "tpz_extern_function({}, {}, {})",
            py_string(&module.identity),
            py_string(&source_name),
            py_span(decl.name.span)
        );
        write_global_assignment(&mut init_body, 8, &info.py_name, &value_py, &source_name);
        exports.insert(source_name, ModuleRuntimeExport::Function { info });
    }
    emit_module_namespace_class(&module.identity, &exports, out);
    emit_module_namespace_assignment(&module.identity, &exports, &mut init_body, 8);
    let init_name = module_init_function_name(&module.identity);
    emit_module_init_function(&init_name, &init_body, "", out);
    Ok((Rc::new(exports), init_name))
}

pub(super) struct RegularImportedModulePreparation<'a> {
    pub(super) ctx: Ctx<'a>,
    pub(super) defining_record_default_helpers: Vec<NominalRecordDefaultHelper<'a>>,
    pub(super) imported_record_runtime_values: Vec<(String, String)>,
    pub(super) receiver_method_module_values: Rc<BTreeMap<String, String>>,
    pub(super) receiver_methods: Vec<ReceiverMethodRegistration>,
    pub(super) protocol_methods: Vec<ProtocolMethodRegistration>,
    pub(super) self_runtime_sources: BTreeMap<String, String>,
    pub(super) init_body: String,
}

pub(super) struct RegularImportedModuleInputs<'a, 'b> {
    pub(super) unit: &'a ResolveOutput,
    pub(super) module: &'a topaz_resolve::ResolvedModule,
    pub(super) checked_aliases: Option<&'a CheckedAliasSurfaces>,
    pub(super) module_default_input_catalog: &'b ModuleDefaultInputCatalog,
    pub(super) module_default_inputs: &'b ModuleDefaultInputFacts,
    pub(super) module_definitions: &'b ModuleDefinitions<'a>,
    pub(super) module_const_values: &'b [(String, Value)],
    pub(super) schema_modules: &'b Rc<JsonSchemaModules<'a>>,
    pub(super) all: &'b BTreeMap<String, ModuleRuntimeExports<'a>>,
    pub(super) hidden_record_runtime_values: Option<&'b BTreeMap<String, Vec<(String, String)>>>,
    pub(super) external_runtime_values: Option<&'b [(String, String)]>,
}

pub(super) fn prepare_regular_imported_module<'a>(
    inputs: &RegularImportedModuleInputs<'a, '_>,
) -> Result<RegularImportedModulePreparation<'a>, PyEmitError> {
    let unit = inputs.unit;
    let module = inputs.module;
    let checked_aliases = inputs.checked_aliases;
    let module_default_input_catalog = inputs.module_default_input_catalog;
    let module_default_inputs = inputs.module_default_inputs;
    let module_definitions = inputs.module_definitions;
    let schema_modules = inputs.schema_modules;
    let all = inputs.all;
    let external_runtime_values = inputs.external_runtime_values;
    let defining_record_default_helpers =
        collect_nominal_record_default_helpers(&module_definitions.records);
    let module_default_import_bindings = &module_default_inputs.imports;
    let mut imported_record_runtime_values =
        collect_selected_imported_runtime_value_py_names_for_defaults(
            module_default_import_bindings,
            all,
            module_default_input_catalog,
        );
    imported_record_runtime_values.extend(
        collect_namespace_imported_runtime_value_py_names_for_defaults(
            module_default_import_bindings,
            all,
            module_default_input_catalog,
        ),
    );
    imported_record_runtime_values.extend(own_exported_runtime_let_py_names_for_defaults(
        &module.identity,
        &module_default_inputs.runtime_names,
    ));

    let module_aliases = checked_aliases.and_then(|aliases| aliases.get(&module.identity));
    let mut ctx = Ctx::new(
        &unit.map,
        module_definitions.records.clone(),
        module_definitions.newtypes.clone(),
        module_definitions.enums.clone(),
        module_aliases,
    );
    ctx.module_identity = &module.identity;
    ctx.method_module_identity = Some(&module.identity);
    ctx.schema_modules = Rc::clone(schema_modules);
    register_protocols(&module_definitions.protocol_names, &mut ctx);
    let receiver_method_module_values = Rc::new(receiver_method_module_value_names(
        &module_default_inputs.module_value_source_names,
        Some(&module.identity),
    ));
    ctx.module_value_py_names = Rc::clone(&receiver_method_module_values);
    register_module_functions(
        &module_definitions.functions,
        Some(&module.identity),
        &module_default_inputs.module_top_bound_names,
        &mut ctx,
    );
    let receiver_methods = prepare_receiver_methods(
        &module_definitions.receiver_impls,
        &module.identity,
        &mut ctx,
    );
    let protocol_methods =
        prepare_protocol_methods(&module_definitions.protocol_impls, &module.identity, &ctx);

    let module_self_runtime_values = module_default_inputs.self_runtime_values.as_ref();
    let mut init_body = String::new();
    write_receiver_method_module_value_seeds(&mut init_body, 8, &receiver_method_module_values);
    emit_receiver_method_registrations(&receiver_methods, 8, &mut init_body);
    emit_protocol_method_registrations(&protocol_methods, 8, &mut init_body);
    write_self_runtime_default_py_seeds(
        &mut init_body,
        8,
        module_self_runtime_values,
        external_runtime_values,
    );
    let self_runtime_sources =
        self_runtime_default_py_source_names(module_self_runtime_values, external_runtime_values);
    for import in module_definitions.imports.iter() {
        emit_import_binding(import, all, &mut ctx, &mut init_body, 8)?;
    }
    enrich_module_function_mutation_metadata(&module_definitions.functions, &mut ctx);
    enrich_module_function_return_metadata(&module_definitions.functions, &mut ctx);
    enrich_nominal_record_default_callable_metadata(&mut ctx);

    Ok(RegularImportedModulePreparation {
        ctx,
        defining_record_default_helpers,
        imported_record_runtime_values,
        receiver_method_module_values,
        receiver_methods,
        protocol_methods,
        self_runtime_sources,
        init_body,
    })
}

pub(super) struct RegularImportedModuleEmission<'a> {
    pub(super) exports: ModuleRuntimeExportMap<'a>,
    pub(super) init_body: String,
}

pub(super) struct ValueBindingInput<'a> {
    pub(super) source_name: &'a str,
    pub(super) mutable: bool,
    pub(super) value: &'a Expr,
    pub(super) annotation: Option<&'a Type>,
    pub(super) runtime_guard: Option<&'a Type>,
    pub(super) span: Span,
}

pub(super) enum ValueBindingStorage<'a> {
    Local,
    Global {
        py_name: String,
        self_runtime_py_name: Option<&'a str>,
    },
}

pub(super) struct ValueBindingEmission {
    pub(super) py_name: String,
    pub(super) cooperative_callback_py_name: Option<String>,
}

pub(super) fn write_value_binding_assignment(
    out: &mut String,
    indent: usize,
    global: bool,
    py_name: &str,
    target_py: &str,
    value_py: &str,
    source_name: &str,
) {
    if global {
        write_global_assignment(out, indent, py_name, value_py, source_name);
    } else {
        writeln!(
            out,
            "{}{target_py} = {value_py}  # {}",
            " ".repeat(indent),
            py_comment_name(source_name)
        )
        .expect("write to string");
    }
}

pub(super) fn emit_value_binding(
    input: ValueBindingInput<'_>,
    storage: ValueBindingStorage<'_>,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<ValueBindingEmission, PyEmitError> {
    let ValueBindingInput {
        source_name,
        mutable,
        value,
        annotation,
        runtime_guard,
        span,
    } = input;
    let (py_name, global, self_runtime_py_name) = match storage {
        ValueBindingStorage::Local => (ctx.new_binding_py_name(source_name), false, None),
        ValueBindingStorage::Global {
            py_name,
            self_runtime_py_name,
        } => (py_name, true, self_runtime_py_name),
    };
    let binding_target = if global {
        format!("globals()[{}]", py_string(&py_name))
    } else {
        ctx.new_binding_target_py_name(&py_name)
    };
    let guarded_value = runtime_guard.map(|_| ctx.fresh_temp("typed_let_value"));
    let value_target = guarded_value.as_deref().unwrap_or(&binding_target);
    let cooperative_callback_py_name =
        cooperative_callback_sibling_py_name_for_value(value, mutable, &py_name, ctx);
    if !emit_contextually_typed_value_expr_to_target_if_needed(
        value,
        annotation,
        value_target,
        ctx,
        indent,
        out,
    )? {
        let value_py = if cooperative_callback_py_name.is_some() {
            ctx.with_cooperative_yields(false, |ctx| {
                emit_contextually_typed_value_expr(value, annotation, ctx)
            })?
        } else {
            emit_contextually_typed_value_expr(value, annotation, ctx)?
        };
        if guarded_value.is_some() {
            writeln!(
                out,
                "{}{value_target} = {value_py}  # {}",
                " ".repeat(indent),
                py_comment_name(source_name)
            )
            .expect("write to string");
        } else {
            write_value_binding_assignment(
                out,
                indent,
                global,
                &py_name,
                &binding_target,
                &value_py,
                source_name,
            );
        }
    }
    if let (Some(runtime_guard), Some(guarded_value)) = (runtime_guard, guarded_value.as_deref()) {
        writeln!(
            out,
            "{}tpz_let_pattern(tpz_type_matches({guarded_value}, {}), {})",
            " ".repeat(indent),
            emit_type_spec_for_typed_let(runtime_guard, ctx)?,
            py_span(span)
        )
        .expect("write to string");
        write_value_binding_assignment(
            out,
            indent,
            global,
            &py_name,
            &binding_target,
            guarded_value,
            source_name,
        );
    }

    if let Some(cooperative_callback_py_name) = cooperative_callback_py_name.as_deref() {
        let value_py = ctx.with_cooperative_yields(true, |ctx| {
            emit_contextually_typed_value_expr(value, annotation, ctx)
        })?;
        let target_py = if global {
            format!("globals()[{}]", py_string(cooperative_callback_py_name))
        } else {
            cooperative_callback_py_name.to_string()
        };
        write_value_binding_assignment(
            out,
            indent,
            global,
            cooperative_callback_py_name,
            &target_py,
            &value_py,
            source_name,
        );
    }
    ctx.register_value_binding(
        source_name,
        mutable,
        value,
        annotation,
        cooperative_callback_py_name
            .clone()
            .map(|py_name| (py_name, false)),
    );
    ctx.set_binding_py_name(source_name, py_name.clone());
    if let Some(self_runtime_py_name) = self_runtime_py_name {
        write_global_assignment(out, indent, self_runtime_py_name, &py_name, source_name);
    }
    Ok(ValueBindingEmission {
        py_name,
        cooperative_callback_py_name,
    })
}

pub(super) fn emit_global_value_binding(
    input: ValueBindingInput<'_>,
    py_name: String,
    self_runtime_py_name: Option<&str>,
    ctx: &mut Ctx<'_>,
    indent: usize,
    out: &mut String,
) -> Result<ValueBindingEmission, PyEmitError> {
    let value = input.value;
    let emission = emit_value_binding(
        input,
        ValueBindingStorage::Global {
            py_name,
            self_runtime_py_name,
        },
        ctx,
        indent,
        out,
    )?;
    note_collection_storage_mutations_in_expr(value, ctx);
    Ok(emission)
}

pub(super) fn emit_regular_imported_module_body<'a>(
    unit: &'a ResolveOutput,
    module: &'a topaz_resolve::ResolvedModule,
    module_const_values: &[(String, Value)],
    module_default_inputs: &ModuleDefaultInputFacts,
    hidden_record_runtime_values: Option<&BTreeMap<String, Vec<(String, String)>>>,
    preparation: RegularImportedModulePreparation<'a>,
    out: &mut String,
) -> Result<RegularImportedModuleEmission<'a>, PyEmitError> {
    let RegularImportedModulePreparation {
        mut ctx,
        defining_record_default_helpers,
        imported_record_runtime_values,
        receiver_method_module_values,
        receiver_methods,
        protocol_methods,
        self_runtime_sources,
        mut init_body,
    } = preparation;
    let module_self_runtime_values = module_default_inputs.self_runtime_values.as_ref();
    let module_runtime_default_names = &module_default_inputs.runtime_names;

    emit_receiver_method_functions(
        &receiver_methods,
        &receiver_method_module_values,
        &mut ctx,
        out,
    )?;
    emit_protocol_method_functions(
        &protocol_methods,
        &receiver_method_module_values,
        &mut ctx,
        out,
    )?;
    let mut exports = ModuleRuntimeExportMap::new();
    for stmt in &module.program.items {
        let exported = matches!(&stmt.kind, StmtKind::Export(_));
        let inner = exported_inner(stmt);
        if stmt_has_bare_return(inner) {
            return Err(PyEmitError::unsupported("return outside a function").at(inner.span));
        }
        match &inner.kind {
            StmtKind::Function(decl) => {
                let source_name = ctx.text(decl.name.span).to_string();
                emit_function(decl, &mut ctx, out)?;
                out.push('\n');
                if exported {
                    let info = ctx
                        .function_info(&source_name)
                        .expect("registered module function")
                        .clone();
                    exports.insert(source_name, ModuleRuntimeExport::Function { info });
                }
            }
            StmtKind::Const { name, ty, value } => {
                let source_name = ctx.text(name.span).to_string();
                let binding = emit_global_value_binding(
                    ValueBindingInput {
                        source_name: &source_name,
                        mutable: false,
                        value,
                        annotation: ty.as_ref(),
                        runtime_guard: None,
                        span: stmt.span,
                    },
                    module_value_name(&module.identity, &source_name),
                    None,
                    &mut ctx,
                    8,
                    &mut init_body,
                )?;
                if exported {
                    let cooperative_callback = if binding.cooperative_callback_py_name.is_none() {
                        ctx.binding_cooperative_callback_target(&source_name, value.span)
                    } else {
                        None
                    };
                    let metadata = ctx.module_value_metadata_for_export(&source_name);
                    exports.insert(
                        source_name,
                        ModuleRuntimeExport::Value {
                            py_name: binding.py_name,
                            cooperative_callback,
                            metadata: Box::new(metadata),
                        },
                    );
                }
            }
            StmtKind::Let {
                mutable,
                pattern,
                ty,
                value,
            } => {
                if !matches!(
                    pattern.kind,
                    PatternKind::Binding(_) | PatternKind::Typed { .. }
                ) {
                    let bound = emit_global_destructuring_let(
                        pattern,
                        *mutable,
                        value,
                        stmt.span,
                        StatementEmission::new(&mut ctx, 8, &mut init_body),
                        |name| module_value_name(&module.identity, name),
                    )?;
                    for (source_name, py_name) in bound {
                        if let Some(default_py_name) =
                            self_runtime_sources.get(source_name.as_str())
                        {
                            write_global_assignment(
                                &mut init_body,
                                8,
                                default_py_name,
                                &py_name,
                                &source_name,
                            );
                        }
                        if exported {
                            let metadata = ctx.module_value_metadata_for_export(&source_name);
                            exports.insert(
                                source_name,
                                ModuleRuntimeExport::Value {
                                    py_name,
                                    cooperative_callback: None,
                                    metadata: Box::new(metadata),
                                },
                            );
                        }
                    }
                    continue;
                }
                let source_name = ctx.binding_name(pattern)?.to_string();
                let binding = emit_global_value_binding(
                    ValueBindingInput {
                        source_name: &source_name,
                        mutable: *mutable,
                        value,
                        annotation: ty.as_ref().or_else(|| pattern_type(pattern)),
                        runtime_guard: (!*mutable).then(|| pattern_type(pattern)).flatten(),
                        span: stmt.span,
                    },
                    module_value_name(&module.identity, &source_name),
                    self_runtime_sources
                        .get(source_name.as_str())
                        .map(String::as_str),
                    &mut ctx,
                    8,
                    &mut init_body,
                )?;
                if exported {
                    let cooperative_callback = if binding.cooperative_callback_py_name.is_none() {
                        ctx.binding_cooperative_callback_target(&source_name, value.span)
                    } else {
                        None
                    };
                    let metadata = ctx.module_value_metadata_for_export(&source_name);
                    exports.insert(
                        source_name,
                        ModuleRuntimeExport::Value {
                            py_name: binding.py_name,
                            cooperative_callback,
                            metadata: Box::new(metadata),
                        },
                    );
                }
            }
            StmtKind::Record(decl) => {
                if exported {
                    let source_name = ctx.text(decl.name.span).to_string();
                    let Some(mut record) = ctx.records.get(&source_name).cloned() else {
                        return Err(
                            PyEmitError::unsupported("imported module export").at(stmt.span)
                        );
                    };
                    let record_self_runtime_values = module_self_runtime_values
                        .get(&source_name)
                        .map(|refs| {
                            refs.iter()
                                .filter(|(name, _)| {
                                    !module_runtime_default_names.exported_values.contains(name)
                                })
                                .cloned()
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let record_hidden_runtime_values = hidden_record_runtime_values
                        .and_then(|records| records.get(&source_name))
                        .cloned()
                        .unwrap_or_default();
                    for field in &mut record.fields {
                        if let Some(default) = &mut field.default
                            && default.helper_py_names.is_none()
                        {
                            default.imported_py = Some(emit_nominal_record_exported_default_expr(
                                default.expr,
                                &unit.map,
                                module_const_values,
                                &imported_record_runtime_values,
                                &record_self_runtime_values,
                                &record_hidden_runtime_values,
                            )?);
                        }
                    }
                    let receiver_methods =
                        exported_receiver_methods_for_nominal(&receiver_methods, &source_name);
                    exports.insert(
                        source_name,
                        ModuleRuntimeExport::Record {
                            record,
                            receiver_methods,
                        },
                    );
                }
            }
            StmtKind::Newtype(decl) => {
                if exported {
                    let source_name = ctx.text(decl.name.span).to_string();
                    let Some(newtype) = ctx.newtypes.get(&source_name).cloned() else {
                        return Err(
                            PyEmitError::unsupported("imported module export").at(stmt.span)
                        );
                    };
                    let receiver_methods =
                        exported_receiver_methods_for_nominal(&receiver_methods, &source_name);
                    exports.insert(
                        source_name,
                        ModuleRuntimeExport::Newtype {
                            newtype,
                            receiver_methods,
                        },
                    );
                }
            }
            StmtKind::Enum(decl) => {
                if exported {
                    let source_name = ctx.text(decl.name.span).to_string();
                    let Some(enum_def) = ctx.enums.get(&source_name).cloned() else {
                        return Err(
                            PyEmitError::unsupported("imported module export").at(stmt.span)
                        );
                    };
                    let receiver_methods =
                        exported_receiver_methods_for_nominal(&receiver_methods, &source_name);
                    exports.insert(
                        source_name,
                        ModuleRuntimeExport::Enum {
                            enum_def,
                            receiver_methods,
                        },
                    );
                }
            }
            StmtKind::TypeAlias(_)
            | StmtKind::Impl(_)
            | StmtKind::Protocol(_)
            | StmtKind::Import(_) => {}
            StmtKind::Return(_) => {
                return Err(PyEmitError::unsupported("return outside a function").at(stmt.span));
            }
            StmtKind::Break { .. } => {
                return Err(PyEmitError::unsupported("break statement shape").at(stmt.span));
            }
            StmtKind::Continue { .. } => {
                return Err(PyEmitError::unsupported("continue statement shape").at(stmt.span));
            }
            _ if exported => {
                return Err(PyEmitError::unsupported("imported module export").at(stmt.span));
            }
            _ => emit_stmt(inner, &mut ctx, 8, &mut init_body)?,
        }
    }

    emit_nominal_record_default_helpers(&defining_record_default_helpers, &mut ctx, out)?;
    Ok(RegularImportedModuleEmission { exports, init_body })
}

pub(super) fn emit_regular_imported_module<'a>(
    inputs: RegularImportedModuleInputs<'a, '_>,
    out: &mut String,
) -> Result<(ModuleRuntimeExports<'a>, String), PyEmitError> {
    let preparation = prepare_regular_imported_module(&inputs)?;
    let RegularImportedModuleInputs {
        unit,
        module,
        module_default_inputs,
        module_const_values,
        hidden_record_runtime_values,
        ..
    } = inputs;
    let RegularImportedModuleEmission {
        exports,
        mut init_body,
    } = emit_regular_imported_module_body(
        unit,
        module,
        module_const_values,
        module_default_inputs,
        hidden_record_runtime_values,
        preparation,
        out,
    )?;
    emit_module_namespace_class(&module.identity, &exports, out);
    emit_module_namespace_assignment(&module.identity, &exports, &mut init_body, 8);
    let init_name = module_init_function_name(&module.identity);
    let fault_suffix = format!(
        " (during initialization of module `{}`; {})",
        module.identity,
        import_chain(unit, &module.identity)
    );
    emit_module_init_function(&init_name, &init_body, &fault_suffix, out);
    Ok((Rc::new(exports), init_name))
}

pub(super) fn emit_imported_module_values<'a>(
    unit: &'a ResolveOutput,
    checked_aliases: Option<&'a CheckedAliasSurfaces>,
    module_default_input_catalog: &ModuleDefaultInputCatalog,
    record_default_const_catalog: &RecordDefaultConstCatalog,
    definition_catalog: &ModuleDefinitionCatalog<'a>,
    schema_modules: Rc<JsonSchemaModules<'a>>,
    out: &mut String,
) -> Result<
    (
        std::collections::BTreeMap<String, ModuleRuntimeExports<'a>>,
        Vec<String>,
    ),
    PyEmitError,
> {
    let mut all = std::collections::BTreeMap::new();
    let mut init_functions = Vec::new();
    let (hidden_record_runtime_values, external_hidden_runtime_values) =
        collect_namespace_private_runtime_default_py_refs(module_default_input_catalog);
    for module in &unit.modules {
        if module.is_entry {
            continue;
        }
        let module_default_inputs = module_default_input_catalog
            .get(&module.identity)
            .expect("imported module default input facts");
        let module_definitions = definition_catalog
            .get(&module.identity)
            .expect("imported module definitions");
        let (exports, init_name) = if module.is_extern {
            emit_extern_imported_module(
                unit,
                module,
                checked_aliases,
                module_default_inputs,
                module_definitions,
                &schema_modules,
                out,
            )?
        } else {
            let module_const_values = record_default_const_catalog
                .get(&module.identity)
                .expect("imported module record-default consts");
            let external_runtime_values = external_hidden_runtime_values
                .get(&module.identity)
                .map(Vec::as_slice);
            emit_regular_imported_module(
                RegularImportedModuleInputs {
                    unit,
                    module,
                    checked_aliases,
                    module_default_input_catalog,
                    module_default_inputs,
                    module_definitions,
                    module_const_values,
                    schema_modules: &schema_modules,
                    all: &all,
                    hidden_record_runtime_values: hidden_record_runtime_values
                        .get(&module.identity),
                    external_runtime_values,
                },
                out,
            )?
        };
        all.insert(module.identity.clone(), exports);
        init_functions.push(init_name);
    }
    Ok((all, init_functions))
}

pub(super) fn emit_module_namespace_class(
    identity: &str,
    exports: &ModuleRuntimeExportMap<'_>,
    out: &mut String,
) {
    let class_name = module_namespace_class_name(identity);
    let runtime_exports = exports
        .iter()
        .filter_map(|(name, export)| export.runtime_py_name().map(|py_name| (name, py_name)))
        .collect::<Vec<_>>();
    out.push_str("@dataclass(frozen=True, slots=True)\n");
    writeln!(out, "class {class_name}:").expect("write to string");
    if runtime_exports.is_empty() {
        out.push_str("    pass\n");
        out.push('\n');
        return;
    }
    for (name, _) in &runtime_exports {
        writeln!(
            out,
            "    {}: object  # {}",
            mangle(name),
            py_comment_name(name)
        )
        .expect("write to string");
    }
    out.push('\n');
}

pub(super) fn emit_module_namespace_assignment(
    identity: &str,
    exports: &ModuleRuntimeExportMap<'_>,
    out: &mut String,
    indent: usize,
) {
    let class_name = module_namespace_class_name(identity);
    let args = exports
        .values()
        .filter_map(ModuleRuntimeExport::runtime_py_name)
        .collect::<Vec<_>>()
        .join(", ");
    let value_py = format!("{class_name}({args})");
    write_global_assignment(
        out,
        indent,
        &module_object_name(identity),
        &value_py,
        identity,
    );
}

pub(super) fn emit_module_init_function(
    name: &str,
    body: &str,
    fault_suffix: &str,
    out: &mut String,
) {
    writeln!(out, "def {name}(host):").expect("write to string");
    out.push_str("    __tpz_defers = []\n");
    emit_defer_helpers(out, 4);
    out.push_str("    try:\n");
    if body.is_empty() {
        out.push_str("        pass\n");
    } else {
        out.push_str(body);
    }
    out.push_str("        __tpz_run_defers()\n");
    out.push_str("    except TpzFault as __tpz_fault:\n");
    writeln!(
        out,
        "        raise TpzFault(__tpz_fault.code, \"{{}}{{}}\".format(__tpz_fault.message, {}), __tpz_fault.span)\n",
        py_string(fault_suffix)
    )
    .expect("write to string");
}

pub(super) fn emit_import_binding<'a>(
    import: &ImportItem,
    module_exports: &std::collections::BTreeMap<String, ModuleRuntimeExports<'a>>,
    ctx: &mut Ctx<'a>,
    out: &mut String,
    indent: usize,
) -> Result<(), PyEmitError> {
    let identity = import_identity(import, ctx);
    let exports = module_exports
        .get(&identity)
        .ok_or_else(|| PyEmitError::unsupported("import target").at(import.span))?;
    match &import.kind {
        ImportKind::Namespace { alias } => {
            let local = alias.as_ref().map_or_else(
                || {
                    ctx.text(
                        import
                            .path
                            .segments
                            .last()
                            .expect("non-empty import path")
                            .span,
                    )
                },
                |alias| ctx.text(alias.span),
            );
            write_global_assignment(
                out,
                indent,
                &mangle(local),
                &module_object_name(&identity),
                local,
            );
            ctx.register_namespace_binding(local);
            Rc::make_mut(&mut ctx.namespaces).insert(local.to_string(), exports.clone());
            for export in exports.values() {
                let methods = match export {
                    ModuleRuntimeExport::Record {
                        receiver_methods, ..
                    }
                    | ModuleRuntimeExport::Newtype {
                        receiver_methods, ..
                    }
                    | ModuleRuntimeExport::Enum {
                        receiver_methods, ..
                    } => Some(receiver_methods),
                    _ => None,
                };
                if let Some(methods) = methods {
                    for (method, info) in methods {
                        ctx.register_receiver_method_info(method, info.clone());
                    }
                }
            }
        }
        ImportKind::Selected { specs } => {
            for spec in specs {
                let source_name = ctx.text(spec.name.span);
                let Some(export) = exports.get(source_name) else {
                    continue;
                };
                let local = spec
                    .alias
                    .as_ref()
                    .map_or(source_name, |alias| ctx.text(alias.span));
                match export {
                    ModuleRuntimeExport::Value {
                        py_name,
                        cooperative_callback,
                        metadata,
                    } => {
                        write_global_assignment(out, indent, &mangle(local), py_name, local);
                        ctx.register_imported_value_binding(
                            local,
                            &mangle(local),
                            cooperative_callback.clone(),
                            metadata.as_ref().clone(),
                        );
                    }
                    ModuleRuntimeExport::Function { info } => {
                        ctx.register_function_info(local, info.clone());
                    }
                    ModuleRuntimeExport::Record {
                        record,
                        receiver_methods,
                    } => {
                        Rc::make_mut(&mut ctx.records).insert(local.to_string(), record.clone());
                        for (method, info) in receiver_methods {
                            ctx.register_receiver_method_info(method, info.clone());
                        }
                    }
                    ModuleRuntimeExport::Newtype {
                        newtype,
                        receiver_methods,
                    } => {
                        Rc::make_mut(&mut ctx.newtypes).insert(local.to_string(), newtype.clone());
                        for (method, info) in receiver_methods {
                            ctx.register_receiver_method_info(method, info.clone());
                        }
                    }
                    ModuleRuntimeExport::Enum {
                        enum_def,
                        receiver_methods,
                    } => {
                        Rc::make_mut(&mut ctx.enums).insert(local.to_string(), enum_def.clone());
                        for (method, info) in receiver_methods {
                            ctx.register_receiver_method_info(method, info.clone());
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

pub(super) fn import_identity_from_map(import: &ImportItem, map: &SourceMap) -> String {
    import
        .path
        .segments
        .iter()
        .map(|segment| text_in_map(map, segment.span))
        .collect::<Vec<_>>()
        .join(".")
}

pub(super) fn import_identity(import: &ImportItem, ctx: &Ctx<'_>) -> String {
    import_identity_from_map(import, ctx.map)
}

pub(super) fn module_namespace_class_name(identity: &str) -> String {
    format!("_tpz_ns_{}", mangle(identity))
}

pub(super) fn module_init_function_name(identity: &str) -> String {
    format!("_tpz_init_{}", mangle(identity))
}

pub(super) fn module_object_name(identity: &str) -> String {
    format!("_tpz_mod_{}", mangle(identity))
}

pub(super) fn module_value_name(identity: &str, name: &str) -> String {
    format!("_tpz_mod_{}__{}", mangle(identity), mangle(name))
}

pub(super) fn nominal_record_default_helper_name(
    module_identity: Option<&str>,
    record_name: &str,
    field_name: &str,
) -> String {
    let module = module_identity.unwrap_or("__entry__");
    format!(
        "_tpz_record_default_{}__{}__{}",
        mangle(module),
        mangle(record_name),
        mangle(field_name)
    )
}
