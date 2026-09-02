use crate::*;

pub(crate) fn emit_multi_module_entry_body(
    unit: &LoweredUnit,
    entry: &Program,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    type_ctx: &TypeCtx<'_>,
    explicit_main: Option<Span>,
    entry_runtime_exports: &[String],
) -> Result<String, EmitError> {
    let plan = entry_import_plan(entry, src)?;
    let mut prelude = String::new();
    let mut seed = Vec::new();
    let mut module_export_surfaces = std::collections::BTreeMap::new();
    for module in unit.modules.iter().filter(|module| !module.is_entry) {
        let surface = export_surface_names(&module.program, &module.text)?;
        module_export_surfaces.insert(module.identity.clone(), surface);
    }

    let mut module_built_exports = std::collections::BTreeMap::new();
    for module in unit.modules.iter().filter(|module| !module.is_entry) {
        let export_surface = module_export_surfaces
            .get(&module.identity)
            .ok_or_else(|| {
                EmitError::unsupported("missing module export surface").at(module.program.span)
            })?;
        let (record, built_exports) = if module.is_extern {
            let (record, exports) =
                emit_extern_namespace_module(&module.identity, &module.program, &module.text)?;
            (record, BuiltRuntimeExportSurface::Extern(exports))
        } else {
            let module_aliases = Aliases::collect(type_ctx, &module.identity);
            let record = emit_namespace_module(
                &module.program,
                &module.text,
                &module_aliases,
                &module_export_surfaces,
                &module_built_exports,
            )?;
            (
                record,
                BuiltRuntimeExportSurface::Module(&export_surface.runtime),
            )
        };
        let suffix = format!(
            "(during initialization of module `{}`; {})",
            module.identity,
            unit.import_chain(&module.identity)
        );
        prelude.push_str(&format!(
            "    let {canonical} = match (async {{ Ok::<Value, RtError>({record}) }}).await {{ \
             Ok(__v) => __v, \
             Err(__e) => return Err(RtError {{ code: __e.code, message: format!(\"{{}} {{}}\", __e.message, {suffix:?}), span: __e.span }}), \
             }};\n",
            canonical = canonical_module(&module.identity),
        ));
        module_built_exports.insert(module.identity.clone(), built_exports);
    }

    for module in unit.modules.iter().filter(|module| !module.is_entry) {
        let Some(import) = plan.get(&module.identity) else {
            continue;
        };
        let canonical = canonical_module(&module.identity);
        match import {
            ImportPlan::Namespace(alias) => {
                prelude.push_str(&format!(
                    "    let {} = {canonical}.clone();\n",
                    mangle(alias)
                ));
                seed.push((alias.clone(), Bind::Namespace));
            }
            ImportPlan::Selected { binds, span } => {
                let export_surface = &module_export_surfaces[&module.identity];
                let built_exports = &module_built_exports[&module.identity];
                for (name, local) in binds {
                    if !selected_import_binds_runtime(export_surface, built_exports, name, *span)? {
                        continue;
                    }
                    prelude.push_str(&format!(
                        "    let {} = member_value_required(&{canonical}, {name:?}, {})?;\n",
                        mangle(local),
                        emit_span(*span),
                    ));
                    seed.push((local.clone(), Bind::Imm));
                }
            }
        }
    }

    emit_entry_body_seeded(
        entry,
        src,
        aliases,
        &prelude,
        &seed,
        EntryFinal::Initialized {
            explicit_main,
            exports: entry_runtime_exports,
        },
    )
}

/// The shared lowering for both shapes: the run entry and its body
/// over the shared `Value`, with the runtime prelude in scope.
pub(crate) fn emit_items(
    unit: &LoweredUnit,
    hybrid: Option<HybridPlan>,
) -> Result<String, EmitError> {
    let entry = unit
        .modules
        .iter()
        .find(|m| m.is_entry)
        .ok_or(EmitError::no_entry())?;
    let src = &entry.text;
    // §17 the unit's cross-module type context (every module's alias table + the
    // namespace map), built once and shared by every `Aliases` view so a qualified
    // type `m.Id` in any module resolves against the exporting module's exports.
    let type_ctx = build_type_ctx(unit, hybrid);
    // §3/§5 the entry module's top-level alias table, threaded down to every
    // typed-annotation `type_test` site.
    let aliases = Aliases::collect(&type_ctx, &entry.identity);
    let has_method_registry = type_ctx.has_method_declarations;
    let explicit_main = unit.explicit_main_span();
    let entry_export_surface = export_surface_names(&entry.program, src)?;
    let entry_runtime_exports = &entry_export_surface.runtime_order;
    let initialize_body = if unit.modules.len() == 1 {
        emit_entry_body(
            &entry.program,
            src,
            &aliases,
            EntryFinal::Initialized {
                explicit_main,
                exports: entry_runtime_exports,
            },
        )?
    } else {
        emit_multi_module_entry_body(
            unit,
            &entry.program,
            src,
            &aliases,
            &type_ctx,
            explicit_main,
            entry_runtime_exports,
        )?
    };
    let export_names = format!("{entry_runtime_exports:?}");
    let runtime_context = if unit.language_version >= topaz_syntax::LangVersion::V5_20 {
        "RtCx::new_module_stable(host)"
    } else {
        "RtCx::new(host)"
    };
    let method_registry = if has_method_registry {
        "// v5.4 user receiver-method registry: `(type id, method) -> closure`.\n\
         type __MethodMap = std::collections::HashMap<&'static str, Value>;\n\
         type __ProtocolNominalMap = std::collections::HashMap<&'static str, __MethodMap>;\n\
         thread_local! {\n\
            static __METHODS: std::cell::RefCell<std::collections::HashMap<&'static str, __MethodMap>> =\n\
                std::cell::RefCell::new(std::collections::HashMap::new());\n\
            static __PROTOCOL_METHODS: std::cell::RefCell<std::collections::HashMap<&'static str, std::collections::HashMap<&'static str, __ProtocolNominalMap>>> =\n\
                std::cell::RefCell::new(std::collections::HashMap::new());\n\
         }\n\
         fn __method_register(id: &'static str, m: &'static str, f: Value) {\n\
            __METHODS.with(|t| t.borrow_mut().entry(id).or_default().insert(m, f));\n\
         }\n\
         fn __method_lookup(id: &str, m: &str) -> Option<Value> {\n\
            __METHODS.with(|t| t.borrow().get(id).and_then(|methods| methods.get(m)).cloned())\n\
         }\n\
         fn __protocol_method_register(module: &'static str, protocol: &'static str, nominal: &'static str, m: &'static str, f: Value) {\n\
            __PROTOCOL_METHODS.with(|t| {\n\
               t.borrow_mut()\n\
                  .entry(module).or_default()\n\
                  .entry(protocol).or_default()\n\
                  .entry(nominal).or_default()\n\
                  .insert(m, f);\n\
            });\n\
         }\n\
         fn __protocol_method_lookup(module: &str, protocol: &str, nominal: &str, m: &str) -> Option<Value> {\n\
            __PROTOCOL_METHODS.with(|t| {\n\
               t.borrow().get(module)\n\
                  .and_then(|protocols| protocols.get(protocol))\n\
                  .and_then(|nominals| nominals.get(nominal))\n\
                  .and_then(|methods| methods.get(m))\n\
                  .cloned()\n\
            })\n\
         }\n"
    } else {
        "// v5.4 no receiver/protocol impls are declared in this emitted unit.\n\
         fn __method_register(_id: &'static str, _m: &'static str, _f: Value) {}\n\
         fn __method_lookup(_id: &str, _m: &str) -> Option<Value> { None }\n\
         fn __protocol_method_register(_module: &'static str, _protocol: &'static str, _nominal: &'static str, _m: &'static str, _f: Value) {}\n\
         fn __protocol_method_lookup(_module: &str, _protocol: &str, _nominal: &str, _m: &str) -> Option<Value> { None }\n"
    };
    let method_clear = if has_method_registry {
        "    __METHODS.with(|t| t.borrow_mut().clear());\n\
         __PROTOCOL_METHODS.with(|t| t.borrow_mut().clear());\n"
    } else {
        ""
    };
    let hybrid_helpers = type_ctx
        .hybrid
        .as_ref()
        .map(|plan| plan.helpers.as_str())
        .unwrap_or("");
    let closure_factories = type_ctx.closure_factories.borrow();
    let entry_initialize_call = if explicit_main.is_some() {
        "__topaz_initialize(cx.clone()).await?"
    } else {
        "__topaz_initialize(cx).await?"
    };
    Ok(format!(
        "use std::rc::Rc;\n\
         use topaz_rt::*;\n\
         \n\
         // §4 (v5.4) the user RECEIVER-METHOD registry: `(type id, method) -> closure`,\n\
         // Rendered as a real TLS registry only when this unit declares impl methods;\n\
         // otherwise the helpers are no-ops so large in-process difftest binaries do\n\
         // not reserve avoidable Windows TLS slots.\n\
         {method_registry}\
         \n\
         pub const TOPAZ_EXPLICIT_MAIN: bool = {explicit};\n\
         \n\
         pub fn topaz_export_names() -> &'static [&'static str] {{\n\
         \x20   &{export_names}\n\
         }}\n\
         \n\
         /// The hostable entry (CDR-006 §4): runs the program on the\n\
         /// calling thread and returns the structured outcome.\n\
         pub fn run_with_host(host: Rc<dyn Host>) -> RunOutcome {{\n\
         \x20   run_with_host_and_input(host, Vec::new(), String::new())\n\
         }}\n\
         \n\
         pub fn run_with_host_and_input(host: Rc<dyn Host>, args: Vec<String>, stdin: String) -> RunOutcome {{\n\
         \x20   let cx = {runtime_context};\n\
         \x20   let __topaz_args = Value::array(args.into_iter().map(Value::str).collect());\n\
         \x20   let __topaz_stdin = Value::str(stdin);\n\
         \x20   match block_on(entry(cx, __topaz_args, __topaz_stdin)) {{\n\
         \x20       Ok(value) => RunOutcome::Completed(value),\n\
         \x20       Err(error) => RunOutcome::Faulted(error),\n\
         \x20   }}\n\
         }}\n\
         \n\
         pub fn call_export_with_host(host: Rc<dyn Host>, name: &str, args: Vec<Value>) -> RunOutcome {{\n\
         \x20   call_export_with_host_and_input(host, name, args, Vec::new(), String::new())\n\
         }}\n\
         \n\
         /// Service-host call seam: deadline expiry drops the in-flight future\n\
         /// before returning, so no detached Topaz evaluation survives it.\n\
         pub fn call_export_with_host_until(\n\
         \x20   host: Rc<dyn Host>,\n\
         \x20   name: &str,\n\
         \x20   args: Vec<Value>,\n\
         \x20   deadline: std::time::Instant,\n\
         ) -> Result<RunOutcome, DeadlineExceeded> {{\n\
         \x20   let outcome = block_on_until(deadline, call_export_with_host_future(host, name, args))?;\n\
         \x20   Ok(match outcome {{\n\
         \x20       Ok(value) => RunOutcome::Completed(value),\n\
         \x20       Err(error) => RunOutcome::Faulted(error),\n\
         \x20   }})\n\
         }}\n\
         \n\
         pub fn call_export_with_host_future(\n\
         \x20   host: Rc<dyn Host>,\n\
         \x20   name: &str,\n\
         \x20   args: Vec<Value>,\n\
         ) -> CallFuture {{\n\
         \x20   let cx = {runtime_context};\n\
         \x20   let __span = Span::new(FileId(0), 0, 0);\n\
         \x20   let name = name.to_string();\n\
         \x20   Box::pin(async move {{\n\
         \x20       __topaz_call_export(cx, &name, args, __span).await\n\
         \x20   }})\n\
         }}\n\
         \n\
         pub fn call_export_with_host_and_input(\n\
         \x20   host: Rc<dyn Host>,\n\
         \x20   name: &str,\n\
         \x20   args: Vec<Value>,\n\
         \x20   _program_args: Vec<String>,\n\
         \x20   _stdin: String,\n\
         ) -> RunOutcome {{\n\
         \x20   let cx = {runtime_context};\n\
         \x20   let __span = Span::new(FileId(0), 0, 0);\n\
         \x20   match block_on(__topaz_call_export(cx, name, args, __span)) {{\n\
         \x20       Ok(value) => RunOutcome::Completed(value),\n\
         \x20       Err(error) => RunOutcome::Faulted(error),\n\
         \x20   }}\n\
         }}\n\
         \n\
         pub fn call_export_json_with_host(host: Rc<dyn Host>, name: &str, args_json: &str) -> String {{\n\
         \x20   call_export_json_with_host_and_input(host, name, args_json, Vec::new(), String::new())\n\
         }}\n\
         \n\
         pub fn call_export_json_with_host_and_input(\n\
         \x20   host: Rc<dyn Host>,\n\
         \x20   name: &str,\n\
         \x20   args_json: &str,\n\
         \x20   program_args: Vec<String>,\n\
         \x20   stdin: String,\n\
         ) -> String {{\n\
         \x20   let args = match canonical_abi_decode_args(args_json) {{\n\
         \x20       Ok(args) => args,\n\
         \x20       Err(error) => return canonical_abi_error(&error),\n\
         \x20   }};\n\
         \x20   match call_export_with_host_and_input(host, name, args, program_args, stdin) {{\n\
         \x20       RunOutcome::Completed(value) => canonical_abi_completed(&value),\n\
         \x20       RunOutcome::Faulted(error) => canonical_abi_faulted(&error),\n\
         \x20   }}\n\
         }}\n\
         \n\
         {hybrid_helpers}\
         {closure_factories}\
         async fn entry(cx: RtCx, __topaz_args: Value, __topaz_stdin: Value) -> Result<Value, RtError> {{\n\
         \x20   let (__topaz_entry_value, _) = {entry_initialize_call};\n\
         {entry_finish}\
         }}\n\
         \n\
         async fn __topaz_exports(cx: RtCx) -> Result<Value, RtError> {{\n\
         \x20   let (_, __exports) = __topaz_initialize(cx).await?;\n\
         \x20   Ok(__exports)\n\
         }}\n\
         \n\
         async fn __topaz_call_export(cx: RtCx, name: &str, args: Vec<Value>, span: Span) -> Result<Value, RtError> {{\n\
         \x20   let __exports = __topaz_exports(cx.clone()).await?;\n\
         \x20   let __callee = member_value_required(&__exports, name, span)?;\n\
         \x20   call_value(__callee, args, cx, span).await\n\
         }}\n\
         \n\
         async fn __topaz_initialize(cx: RtCx) -> Result<(Value, Value), RtError> {{\n\
         \x20   let _ = &cx;\n\
         {method_clear}\
         {initialize_body}\
         }}\n",
        explicit = explicit_main.is_some(),
        entry_finish = explicit_main.map_or_else(
            || "    Ok(__topaz_entry_value)\n".to_string(),
            |span| format!(
                "    call_value(__topaz_entry_value, vec![__topaz_args, __topaz_stdin], cx, {}).await\n",
                emit_span(span),
            ),
        ),
    ))
}

/// As [`emit_entry_body`], but with a PRELUDE emitted before the program's own
/// lowering and SEED locals already in scope — used by the multi-module path to
/// bind each imported namespace module (lowered to a record of its exports) under
/// its alias before the entry body runs.
pub(crate) fn emit_entry_body_seeded(
    program: &Program,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    prelude: &str,
    seed: &[(String, Bind)],
    final_mode: EntryFinal<'_>,
) -> Result<String, EmitError> {
    // GUARANTEE every coverage gap raised while lowering this program body carries
    // a span: the program is the outermost fallback location (first-wins, so a
    // tighter span from `emit_expr` and friends is preserved). Statement-structural
    // refusals (redeclaration, top-level `return`, …) currently land here; their
    // per-statement precision is refined as coverage grows (CDR-001 §5 TPZ6001).
    emit_entry_body_seeded_inner(program, src, aliases, prelude, seed, final_mode)
        .map_err(|e| e.at(program.span))
}

pub(crate) fn emit_entry_body_seeded_inner(
    program: &Program,
    src: &LoweredText,
    aliases: &Aliases<'_, '_>,
    prelude: &str,
    seed: &[(String, Bind)],
    final_mode: EntryFinal<'_>,
) -> Result<String, EmitError> {
    // §17 entry value exports (`export function` / `export const` /
    // `export let`) execute exactly like their unexported forms in the ordinary
    // `run_with_host` path, but the emitted module also needs a host-callable export
    // dispatcher. Normalize each top-level `Export(inner)` to its inner declaration
    // BEFORE the const-hoist / top-function-seeding / statement passes (which all match on
    // `stmt.kind`), then let [`EntryFinal::Initialized`] materialize the runtime
    // export record after initialization. An `export type` unwraps to its `TypeAlias`,
    // a runtime no-op. (A non-export statement is cloned through unchanged; the entry
    // body already clones into `rest` below.)
    let normalized: Vec<Stmt> = program
        .items
        .iter()
        .map(|s| match &s.kind {
            StmtKind::Export(inner) => (**inner).clone(),
            _ => s.clone(),
        })
        .collect();
    let (stmts, tail) = split_tail(&normalized);
    // §7 a TOP-LEVEL `return` (outside any function/lambda) is refused — the
    // interpreter runtime-faults it ("return outside a function"). Declining
    // here lets `emit_stmt_seq`'s `Return` arm emit `return Ok(e)`
    // unconditionally, since it is then reached only inside a function/lambda
    // body.
    if let Some(stmt) = stmts.iter().find(|s| stmt_has_bare_return(s)) {
        return Err(EmitError::unsupported("return outside a function").at(stmt.span));
    }
    if let Some(tail) = tail.filter(|t| expr_has_bare_return(t)) {
        return Err(EmitError::unsupported("return outside a function").at(tail.span));
    }
    // §4/§17 TOP-LEVEL `const` bindings are evaluated by the interpreter's load-time
    // const pass BEFORE the main statement sequence — and, in a multi-module unit,
    // BEFORE any `import` is executed (`run_unit` runs each module's const pass ahead
    // of binding its imports). Emit them HOISTED, in textual order, as immutable
    // `let`s; each sees the earlier consts (a const expression may reference an
    // earlier const, never a non-const binding) but NOT the imported namespace
    // aliases (the `seed`): a const that references an imported member is a free
    // identifier here and refuses, exactly as the interpreter's const pass rejects it
    // (TPZ5001 — it cannot see an as-yet-unbound import, and const-eval rejects a
    // member expression). (A block-local const is NOT hoisted — it is an in-place
    // `let`, handled by `emit_stmt_seq`.) A duplicate const name — or one colliding
    // with an alias — is a redeclaration.
    let mut const_scope: Vec<(String, Bind)> = Vec::new();
    let mut const_values = ConstValues::new();
    let mut const_lines = String::new();
    let mut rest: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for stmt in stmts {
        if let StmtKind::Const { name, value, .. } = &stmt.kind {
            let var = text(src, name.span);
            if const_values.contains_key(var) || seed.iter().any(|(n, _)| n == var) {
                return Err(EmitError::unsupported("same-scope redeclaration").at(name.span));
            }
            if !const_initializer_ok(value, src, &const_values) {
                // The interpreter's const pass would fault this (TPZ5001); refuse
                // rather than lower a member/call/prelude-fallback that diverges.
                return Err(EmitError::unsupported("non-constant const initializer").at(value.span));
            }
            match const_eval_emit(value, src, &const_values) {
                Ok(v) => {
                    let value_rs = emit_expr(value, src, aliases, &const_scope, false)?;
                    const_lines.push_str(&format!("    let {} = {value_rs};\n", mangle(var)));
                    const_values.insert(var.to_string(), v);
                }
                Err(e) => {
                    // The interpreter's const pass faults HERE; emit its const-guard fault so
                    // the binary faults identically (the ENTRY has no module-init suffix, so it
                    // matches exactly). `let … = return …` keeps the name defined (type `!`).
                    const_lines.push_str(&format!(
                        "    let {}: Value = return Err(fault(codes::GUARD_TYPE, {:?}, {}));\n",
                        mangle(var),
                        e.message,
                        emit_span(e.span),
                    ));
                    const_values.insert(var.to_string(), Value::Unit);
                }
            }
            const_scope.push((var.to_string(), Bind::Imm));
        } else {
            rest.push(stmt.clone());
        }
    }
    // Assemble the entry scope: the imported names (the `seed` — namespace aliases
    // and/or selected locals) followed by the hoisted consts. `base = 0`: the imports,
    // the consts, AND the entry's top-level `let`s are ALL ONE scope (the interpreter
    // binds imports and top-level bindings into the entry's single env), so a later
    // `let`/`const` reusing an imported OR const name is a SAME-SCOPE REDECLARATION
    // (refused, as the interpreter faults it) — NOT a shadow. A nested block/function
    // body is a child scope and may still shadow (handled by `emit_stmt_seq`'s own
    // child scopes, unaffected by `base`).
    //
    // Emission order is PRELUDE → entry CONST lowering → body, matching `run_unit`:
    // every NON-ENTRY module is fully initialized — its own const pass AND its
    // top-level effects (the prelude builds each module's record, running them) —
    // BEFORE the entry module's const pass runs. So an imported module's effect runs
    // before an entry faulting const, as in the interpreter. (The entry consts are
    // still LOWERED above in a const-only scope WITHOUT the imports, so a const reading
    // an import refuses — that gate is independent of this emission order; the entry's
    // own const pass still precedes its import BINDING, which emits nothing.)
    let mut locals: Vec<(String, Bind)> = seed.to_vec();
    locals.extend(const_scope);
    let base = 0;
    // §7 seed a forward-reference cell for each top-level function BEFORE any
    // statement, so a body that names a function declared later — even across
    // non-function statements — resolves. The cell is `None` until its declaration
    // runs (`top_cell_set`); a read before then faults `GUARD_UNBOUND`, matching the
    // interpreter's positional binding. A top-level function shadowing a PRELUDE
    // name is REFUSED (its prelude-vs-user resolution is dynamic; see
    // `is_prelude_name`). A name colliding with an import/const is left to the
    // `Function` arm's redeclaration check (skip seeding it).
    refuse_prelude_named_top_functions(&rest, src)?;
    let mut top_fn_seed = String::new();
    for stmt in &rest {
        if let StmtKind::Function(decl) = &stmt.kind {
            let fname = text(src, decl.name.span);
            // A DUPLICATE top-level function (already seeded a top cell) is a
            // same-scope redeclaration — refuse, so the `Function` arm's `is_top_fn`
            // path cannot silently `top_cell_set` twice (the resolver's TPZ3008; this
            // also guards the `--unchecked` lane).
            if locals
                .iter()
                .any(|(n, b)| n == fname && matches!(b, Bind::TopFnCell))
            {
                return Err(EmitError::unsupported("same-scope redeclaration").at(decl.name.span));
            }
            // A name colliding with an import/const stays positional — the `Function`
            // arm's own redeclaration check handles it.
            if locals.iter().any(|(n, _)| n == fname) {
                continue;
            }
            top_fn_seed.push_str(&format!("    let {} = top_cell();\n", mangle(fname)));
            locals.push((fname.to_string(), Bind::TopFnCell));
        }
    }
    let top_value_seed = seed_top_runtime_value_cells(&rest, src, &mut locals)?;
    // §4 (v5.4) build the user RECEIVER-METHOD registry: lower each method to a
    // closure (capturing the top-level functions it calls — already TopFnCells in
    // `locals`) and `__method_register` it under `(type id, method)`. Emitted AFTER
    // the function cells are seeded so a method body that calls a sibling function /
    // method resolves (the cells are FILLED during the body run; the closures capture
    // the cells by Rc, so invocation-time reads see the filled value). A method's name
    // collision / coherence / `self`-first errors were already reported by the
    // checker; the emitter only lowers a well-typed program (the `--unchecked` lane
    // dispatches on the runtime nominal id, matching the interpreter byte-for-byte).
    let mut method_seed = String::new();
    let mut method_ids: Vec<&String> = aliases.methods.keys().collect();
    method_ids.sort_unstable(); // deterministic emit
    for type_id in method_ids {
        for m in &aliases.methods[type_id] {
            let mname = text(src, m.decl.name.span);
            let closure = emit_method_closure(&m.decl, src, aliases, &locals)?;
            method_seed.push_str(&emitted_method_registration(
                aliases.runtime_identity(),
                type_id,
                mname,
                &closure,
            ));
        }
    }
    let (lines, result) = emit_stmt_seq(StatementSequenceEmission {
        stmts: &rest,
        tail,
        src,
        aliases,
        locals: &mut locals,
        base,
        in_loop: false,
        defer_scope: false,
        at_module_top: true,
    })?;
    let final_result = match final_mode {
        EntryFinal::Initialized {
            explicit_main,
            exports,
        } => {
            let fields = runtime_export_fields(exports, &locals);
            let entry_value = explicit_main.map_or_else(
                || "__topaz_init_value".to_string(),
                |span| {
                    format!(
                        "top_cell_get(&{}, \"main\", {})?",
                        mangle("main"),
                        emit_span(span),
                    )
                },
            );
            format!(
                "    let __topaz_init_value = {result};\n\
                 \x20   let __topaz_entry_value = {entry_value};\n\
                 \x20   Ok((__topaz_entry_value, Value::record([{}])))\n",
                fields.join(", ")
            )
        }
    };
    let self_default_seed = self_runtime_default_seed_lines(aliases);
    Ok(format!(
        "{prelude}{const_lines}{self_default_seed}{top_fn_seed}{top_value_seed}{method_seed}{lines}{final_result}"
    ))
}
