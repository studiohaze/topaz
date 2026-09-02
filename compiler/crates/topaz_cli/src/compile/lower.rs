use crate::*;

pub(super) fn resolve_and_lower_package_with_report(
    target: &PackageTarget,
    unchecked: bool,
    backend: Backend,
    native_report: Option<&mut NativeReportSession>,
    command: &'static str,
    build_target: &str,
) -> Result<GeneratedSource, ExitCode> {
    let unit = resolve_package_target(target);
    for diag in &unit.diagnostics {
        eprintln!("{}", render(diag, &unit.map));
    }
    if has_errors(&unit.diagnostics) {
        return Err(ExitCode::FAILURE);
    }
    let checked = if unchecked {
        None
    } else {
        match check_resolved_unit(&unit, false, target.version) {
            Ok(checked) => Some(checked),
            Err(n) => {
                eprintln!(
                    "{}: {n} type diagnostic{}",
                    target.entry,
                    if n == 1 { "" } else { "s" }
                );
                return Err(ExitCode::FAILURE);
            }
        }
    };
    lower_resolved_package_with_report(
        target,
        &unit,
        checked.as_ref(),
        backend,
        native_report,
        command,
        build_target,
    )
}

pub(super) fn lower_resolved_package_with_report(
    target: &PackageTarget,
    unit: &topaz_resolve::ResolveOutput,
    checked: Option<&topaz_check::CheckedUnit>,
    backend: Backend,
    native_report: Option<&mut NativeReportSession>,
    command: &'static str,
    build_target: &str,
) -> Result<GeneratedSource, ExitCode> {
    let lispex_application = match checked {
        Some(checked) => checked_lispex_application_plan(target, checked)?,
        None if target.generated_std_modules.is_empty() => None,
        None => {
            eprintln!(
                "topaz: the Lispex application surface requires checked reachability; drop `--unchecked`"
            );
            return Err(ExitCode::FAILURE);
        }
    };
    let lowered = lower_rust_input(unit, checked)?;
    if backend == Backend::Native {
        let native_input = topaz_emit::NativeInput { unit: &lowered };
        if !unit.modules.iter().any(|module| module.is_extern)
            && let Ok(outcome) = topaz_emit::emit_native_or_hybrid(&native_input)
        {
            if let Some(report) = native_report {
                report.capture(command, build_target, target.version, outcome.decision);
            }
            let compiler = rust_compiler_provenance(unit, &outcome.rust).map_err(|error| {
                eprintln!("topaz: cannot record compiler provenance: {error}");
                ExitCode::FAILURE
            })?;
            return Ok(GeneratedSource {
                text: outcome.rust,
                compiler,
                lispex_application,
                explicit_main: topaz_resolve::has_explicit_main(unit),
            });
        }
    }
    let text = topaz_emit::emit_module(&lowered).map_err(|e| {
        match e.diagnostic() {
            Some(diag) => eprintln!("{}", render(&diag, &unit.map)),
            None => eprintln!("topaz: cannot compile this program yet — {e}"),
        }
        ExitCode::FAILURE
    })?;
    let compiler = rust_compiler_provenance(unit, &text).map_err(|error| {
        eprintln!("topaz: cannot record compiler provenance: {error}");
        ExitCode::FAILURE
    })?;
    Ok(GeneratedSource {
        text,
        compiler,
        lispex_application,
        explicit_main: topaz_resolve::has_explicit_main(unit),
    })
}

pub(super) struct GeneratedSource {
    pub(super) text: String,
    pub(super) compiler: artifact::CompilerProvenance,
    pub(super) lispex_application: Option<topaz_lispex_product::CheckedApplicationPlan>,
    pub(super) explicit_main: bool,
}

pub(super) type ExportedRecords = BTreeMap<String, topaz_check::unit::ExportedRecord>;
pub(super) type ExportedEnums = BTreeMap<String, topaz_check::unit::ExportedEnum>;
pub(super) type ExportedNewtypes = BTreeMap<String, topaz_check::unit::ExportedNewtype>;

pub(super) struct WebLowered {
    pub(super) rust: String,
    pub(super) compiler: artifact::CompilerProvenance,
    pub(super) entry_exports: topaz_check::ModuleExports,
    pub(super) records: ExportedRecords,
    pub(super) enums: ExportedEnums,
    pub(super) newtypes: ExportedNewtypes,
}

pub(super) struct ServiceLowered {
    pub(super) rust: String,
    pub(super) compiler: artifact::CompilerProvenance,
    pub(super) entry_exports: topaz_check::ModuleExports,
}

pub(super) fn stable_self_type_id(origin: &str) -> u32 {
    origin
        .as_bytes()
        .iter()
        .fold(2_166_136_261_u32, |hash, byte| {
            hash.wrapping_mul(16_777_619) ^ u32::from(*byte)
        })
}

pub(super) fn self_semantic_type(
    semantic: &topaz_hir::SemanticType,
) -> Result<topaz_check::Type, String> {
    use topaz_check::{Ctor, Lit, Prim, Type};
    use topaz_hir::{
        SemanticConstructor as C, SemanticLiteral as L, SemanticPrimitive as P, SemanticType as S,
    };
    let nested = |values: &[S]| {
        values
            .iter()
            .map(self_semantic_type)
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(match semantic {
        S::Primitive(value) => Type::Prim(match value {
            P::Int => Prim::Int,
            P::Float => Prim::Float,
            P::String => Prim::String,
            P::Bool => Prim::Bool,
            P::Unit => Prim::Unit,
        }),
        S::Literal(value) => Type::Literal(match value {
            L::String(value) => Lit::Str(value.clone()),
            L::Int(value) => Lit::Int(*value),
            L::Float(value) => Lit::Float(value.clone()),
            L::Bool(value) => Lit::Bool(*value),
            L::Null => Lit::Null,
        }),
        S::Union(values) => Type::Union(nested(values)?),
        S::Record(fields) => Type::Record(
            fields
                .iter()
                .map(|field| Ok((field.name.clone(), self_semantic_type(&field.ty)?)))
                .collect::<Result<Vec<_>, String>>()?,
        ),
        S::Constructor {
            constructor,
            arguments,
        } => Type::Ctor(
            match constructor {
                C::Array => Ctor::Array,
                C::Map => Ctor::Map,
                C::Set => Ctor::Set,
                C::Option => Ctor::Option,
                C::Result => Ctor::Result,
                C::Range => Ctor::Range,
            },
            nested(arguments)?,
        ),
        S::Function {
            parameters,
            variadic,
            result,
        } => Type::Func {
            params: nested(parameters)?,
            variadic: variadic
                .as_deref()
                .map(self_semantic_type)
                .transpose()?
                .map(Box::new),
            ret: Box::new(self_semantic_type(result)?),
        },
        S::Foreign {
            identity,
            arguments,
        } => Type::Foreign {
            name: identity.clone(),
            args: nested(arguments)?,
        },
        S::Rigid { name, origin } => Type::Skolem {
            name: name.clone(),
            id: stable_self_type_id(origin),
            origin: origin.clone(),
        },
        S::Template => Type::Template,
        S::File => Type::File,
        S::JsonValue => Type::JsonValue,
        S::Bytes => Type::Bytes,
        S::ByteBuffer => Type::ByteBuffer,
        S::Path => Type::Path,
        S::Regex => Type::Regex,
        S::Match => Type::Match,
        S::TomlValue => Type::TomlValue,
        S::Url => Type::Url,
        S::Date => Type::Date,
        S::BigInt => Type::BigInt,
        S::Decimal => Type::Decimal,
        S::RoundingMode => Type::RoundingMode,
        S::Enum {
            identity,
            arguments,
        } => Type::Enum {
            base: identity.clone(),
            args: nested(arguments)?,
        },
        S::NominalRecord {
            identity,
            arguments,
        } => Type::NominalRecord {
            base: identity.clone(),
            args: nested(arguments)?,
        },
        S::Newtype {
            identity,
            arguments,
        } => Type::Newtype {
            base: identity.clone(),
            args: nested(arguments)?,
        },
        S::Unknown | S::InferenceVariable => {
            return Err("self target adapter refuses an incomplete semantic type".to_string());
        }
    })
}

pub(super) fn self_target_module_exports(
    facts: &topaz_self_frontend::SelfTargetAdapterFacts,
) -> Result<topaz_check::ModuleExports, String> {
    if facts.schema != topaz_self_frontend::SELF_TARGET_ADAPTER_FACTS_SCHEMA
        || facts.producer != "topaz-stage2"
    {
        return Err("self target adapter fact provenance is invalid".to_string());
    }
    let mut surface = topaz_check::ModuleExports::default();
    for fact in &facts.exports {
        let required = fact
            .parameter_defaults
            .iter()
            .position(|defaulted| *defaulted)
            .unwrap_or(fact.parameter_names.len());
        surface.values.insert(
            fact.name.clone(),
            topaz_check::ExportedValue {
                ty: self_semantic_type(&fact.ty)?,
                vars: u32::try_from(fact.type_parameters)
                    .map_err(|_| "self target type parameter count exceeds u32".to_string())?,
                bounds: vec![Vec::new(); fact.type_parameters],
                required,
                names: fact.parameter_names.clone(),
                names_known: fact.names_known,
                defaulted: fact.parameter_defaults.clone(),
                nominals: topaz_check::unit::ExportedNominals::default(),
            },
        );
    }
    Ok(surface)
}

pub(super) fn lower_self_product_for_web(
    product: topaz_self_frontend::SelfCompilationProduct,
) -> Result<WebLowered, ExitCode> {
    let facts = topaz_self_frontend::project_target_adapter_facts(&product).map_err(|error| {
        eprintln!("topaz: self Web target facts are incomplete: {error}");
        ExitCode::FAILURE
    })?;
    let entry_exports = self_target_module_exports(&facts).map_err(|error| {
        eprintln!("topaz: self Web target facts are invalid: {error}");
        ExitCode::FAILURE
    })?;
    let generated = completed_self_generated_source_with_facts(product, &facts)?;
    Ok(WebLowered {
        rust: generated.text,
        compiler: generated.compiler,
        entry_exports,
        records: BTreeMap::new(),
        enums: BTreeMap::new(),
        newtypes: BTreeMap::new(),
    })
}

pub(super) fn lower_self_product_for_service(
    product: topaz_self_frontend::SelfCompilationProduct,
) -> Result<ServiceLowered, ExitCode> {
    let facts = topaz_self_frontend::project_target_adapter_facts(&product).map_err(|error| {
        eprintln!("topaz: self service target facts are incomplete: {error}");
        ExitCode::FAILURE
    })?;
    let entry_exports = self_target_module_exports(&facts).map_err(|error| {
        eprintln!("topaz: self service target facts are invalid: {error}");
        ExitCode::FAILURE
    })?;
    let generated = completed_self_generated_source_with_facts(product, &facts)?;
    Ok(ServiceLowered {
        rust: generated.text,
        compiler: generated.compiler,
        entry_exports,
    })
}

pub(super) fn validate_http_service_handler(lowered: &ServiceLowered) -> Result<(), String> {
    use topaz_check::Type;

    let handle = lifecycle_export(&lowered.entry_exports, "handle", 1)?;
    let Type::Func {
        params,
        variadic: None,
        ret,
    } = &handle.ty
    else {
        return Err(
            "`handle` must have type `(std.http.HttpRequest) -> std.http.HttpResponse`".into(),
        );
    };
    let request_ok = matches!(
        params.as_slice(),
        [Type::NominalRecord { base, args }]
            if args.is_empty() && nominal_base_is(base, "HttpRequest")
    );
    let response_ok = matches!(
        ret.as_ref(),
        Type::NominalRecord { base, args }
            if args.is_empty() && nominal_base_is(base, "HttpResponse")
    );
    if !request_ok || !response_ok {
        return Err(
            "`handle` must have type `(std.http.HttpRequest) -> std.http.HttpResponse`".into(),
        );
    }
    Ok(())
}

pub(super) fn resolve_and_lower_package_for_service(
    target: &PackageTarget,
) -> Result<ServiceLowered, ExitCode> {
    let unit = resolve_package_target(target);
    for diag in &unit.diagnostics {
        eprintln!("{}", render(diag, &unit.map));
    }
    if has_errors(&unit.diagnostics) {
        return Err(ExitCode::FAILURE);
    }
    let checked = match check_resolved_unit(&unit, false, target.version) {
        Ok(checked) => checked,
        Err(n) => {
            eprintln!(
                "{}: {n} type diagnostic{}",
                target.entry,
                if n == 1 { "" } else { "s" }
            );
            return Err(ExitCode::FAILURE);
        }
    };
    reject_reached_lispex_application_target(target, &checked, "http-service")?;
    let entry_identity = unit
        .modules
        .iter()
        .find(|module| module.is_entry)
        .map(|module| module.identity.as_str())
        .unwrap_or("");
    let entry_exports = checked
        .exports
        .get(entry_identity)
        .cloned()
        .unwrap_or_default();
    let rust = lower_checked_unit_with_report(
        &unit,
        target.version,
        Backend::Boxed,
        Some(&checked),
        None,
        "service",
        "native",
    )?;
    let compiler = rust_compiler_provenance(&unit, &rust).map_err(|error| {
        eprintln!("topaz: cannot record compiler provenance: {error}");
        ExitCode::FAILURE
    })?;
    Ok(ServiceLowered {
        rust,
        compiler,
        entry_exports,
    })
}

pub(super) fn validate_web_app_lifecycle(
    lowered: &WebLowered,
    lifecycle: topaz_package::WebLifecycle,
) -> Result<(), String> {
    use topaz_check::Type;

    let init = lifecycle_export(&lowered.entry_exports, "init", 0)?;
    let update = lifecycle_export(&lowered.entry_exports, "update", 3)?;
    let view = lifecycle_export(&lowered.entry_exports, "view", 1)?;
    let step_name = match lifecycle {
        topaz_package::WebLifecycle::V1 => "AppStep",
        topaz_package::WebLifecycle::V2 => "WebAppStep",
    };

    let Type::Func {
        params: init_params,
        variadic: None,
        ret: init_ret,
    } = &init.ty
    else {
        return Err(format!(
            "`init` must have type `() -> {step_name}<Model, Msg>`"
        ));
    };
    if !init_params.is_empty() {
        return Err("`init` must not accept parameters".into());
    }
    let Type::NominalRecord {
        base: init_base,
        args: init_args,
    } = init_ret.as_ref()
    else {
        return Err(format!(
            "`init` must return `{step_name}<Model, Msg>` for Web lifecycle {}",
            lifecycle.as_str()
        ));
    };
    if init_args.len() != 2 || !nominal_base_is(init_base, step_name) {
        return Err(format!(
            "`init` must return `std.dom.{step_name}<Model, Msg>` for Web lifecycle {}",
            lifecycle.as_str()
        ));
    }
    let model = &init_args[0];
    let message = &init_args[1];

    let Type::Func {
        params: update_params,
        variadic: None,
        ret: update_ret,
    } = &update.ty
    else {
        return Err("`update` must be a non-variadic function".into());
    };
    let event_matches = match lifecycle {
        topaz_package::WebLifecycle::V1 => matches!(
            update_params.get(2),
            Some(Type::NominalRecord { base, args })
                if args.is_empty() && nominal_base_is(base, "BrowserEvent")
        ),
        topaz_package::WebLifecycle::V2 => matches!(
            update_params.get(2),
            Some(Type::Enum { base, args })
                if args.is_empty() && nominal_base_is(base, "WebAppEvent")
        ),
    };
    if update_params.len() != 3
        || &update_params[0] != model
        || &update_params[1] != message
        || !event_matches
    {
        let event_name = match lifecycle {
            topaz_package::WebLifecycle::V1 => "BrowserEvent",
            topaz_package::WebLifecycle::V2 => "WebAppEvent",
        };
        return Err(format!(
            "`update` must have type `(Model, Msg, {event_name}) -> {step_name}<Model, Msg>` for Web lifecycle {}",
            lifecycle.as_str()
        ));
    }
    if update_ret.as_ref() != init_ret.as_ref() {
        return Err(format!(
            "`update` must return the same `{step_name}<Model, Msg>` as `init`"
        ));
    }

    let Type::Func {
        params: view_params,
        variadic: None,
        ret: view_ret,
    } = &view.ty
    else {
        return Err("`view` must be a non-variadic function".into());
    };
    if view_params.as_slice() != [model.clone()] {
        return Err("`view` must accept the same `Model` used by `init` and `update`".into());
    }
    let Type::Enum {
        base: html_base,
        args: html_args,
    } = view_ret.as_ref()
    else {
        return Err("`view` must return `Html<Msg>`".into());
    };
    if html_args.as_slice() != [message.clone()] || !nominal_base_is(html_base, "Html") {
        return Err("`view` must return `std.dom.Html<Msg>` using the lifecycle `Msg`".into());
    }

    for (label, ty, value) in [("Model", model, init), ("Msg", message, init)] {
        let records = scoped_exported_records(&lowered.records, value);
        let enums = scoped_exported_enums(&lowered.enums, value);
        let newtypes = scoped_exported_newtypes(&lowered.newtypes, value);
        let rendered = ts_abi_type(ty, &records, &enums, &newtypes);
        if rendered.contains("TopazUnsupported") || rendered == "TopazAbiValue" {
            return Err(format!(
                "lifecycle `{label}` type `{ty}` is not eligible for the checked Web ABI"
            ));
        }
    }
    Ok(())
}

pub(super) fn lifecycle_export<'a>(
    surface: &'a topaz_check::ModuleExports,
    name: &str,
    arity: usize,
) -> Result<&'a topaz_check::ExportedValue, String> {
    let Some(value) = surface.values.get(name) else {
        return Err(format!("missing required exported function `{name}`"));
    };
    if value.vars != 0 {
        return Err(format!("`{name}` must not be generic"));
    }
    if !value.names_known || value.required != arity || value.defaulted.iter().any(|v| *v) {
        return Err(format!(
            "`{name}` must declare exactly {arity} required parameter(s)"
        ));
    }
    Ok(value)
}

pub(super) fn nominal_base_is(base: &str, expected: &str) -> bool {
    base == expected
        || base == format!("std.dom::{expected}")
        || base.ends_with(&format!(".{expected}"))
}

pub(super) fn lower_checked_unit(
    unit: &topaz_resolve::ResolveOutput,
    version: LangVersion,
    backend: Backend,
    checked: Option<&topaz_check::CheckedUnit>,
) -> Result<String, ExitCode> {
    lower_checked_unit_with_report(unit, version, backend, checked, None, "build", "native")
}

pub(super) fn lower_checked_unit_with_report(
    unit: &topaz_resolve::ResolveOutput,
    version: LangVersion,
    backend: Backend,
    checked: Option<&topaz_check::CheckedUnit>,
    native_report: Option<&mut NativeReportSession>,
    command: &'static str,
    build_target: &str,
) -> Result<String, ExitCode> {
    let lowered = lower_rust_input(unit, checked)?;
    if backend == Backend::Native {
        let native_input = topaz_emit::NativeInput { unit: &lowered };
        if let Ok(outcome) = topaz_emit::emit_native_or_hybrid(&native_input) {
            if let Some(report) = native_report {
                report.capture(command, build_target, version, outcome.decision);
            }
            return Ok(outcome.rust);
        }
    }
    topaz_emit::emit_module(&lowered).map_err(|e| {
        match e.diagnostic() {
            Some(diag) => eprintln!("{}", render(&diag, &unit.map)),
            None => eprintln!("topaz: cannot compile this program yet — {e}"),
        }
        ExitCode::FAILURE
    })
}

pub(super) fn lower_rust_input(
    unit: &topaz_resolve::ResolveOutput,
    checked: Option<&topaz_check::CheckedUnit>,
) -> Result<topaz_hir::LoweredUnit, ExitCode> {
    let result = match checked {
        Some(checked) => topaz_lower::lower_checked(unit, checked),
        None => topaz_lower::lower_resolved_compat(unit),
    };
    result.map_err(|error| {
        eprintln!("topaz: cannot construct checked Lowered IR — {error}");
        ExitCode::FAILURE
    })
}

pub(super) fn collect_exported_records(
    exports: &BTreeMap<String, topaz_check::ModuleExports>,
    preferred: &topaz_check::ModuleExports,
) -> ExportedRecords {
    let mut records = BTreeMap::new();
    for (name, record) in &preferred.records {
        records.insert(name.clone(), record.clone());
    }
    for surface in exports.values() {
        for (name, record) in &surface.records {
            records
                .entry(name.clone())
                .or_insert_with(|| record.clone());
        }
    }
    records
}

pub(super) fn collect_exported_enums(
    exports: &BTreeMap<String, topaz_check::ModuleExports>,
    preferred: &topaz_check::ModuleExports,
) -> ExportedEnums {
    let mut enums = BTreeMap::new();
    for (name, enm) in &preferred.enums {
        enums.insert(name.clone(), enm.clone());
    }
    for surface in exports.values() {
        for (name, enm) in &surface.enums {
            enums.entry(name.clone()).or_insert_with(|| enm.clone());
        }
    }
    enums
}

pub(super) fn collect_exported_newtypes(
    exports: &BTreeMap<String, topaz_check::ModuleExports>,
    preferred: &topaz_check::ModuleExports,
) -> ExportedNewtypes {
    let mut newtypes = BTreeMap::new();
    for (name, newtype) in &preferred.newtypes {
        newtypes.insert(name.clone(), newtype.clone());
    }
    for surface in exports.values() {
        for (name, newtype) in &surface.newtypes {
            newtypes
                .entry(name.clone())
                .or_insert_with(|| newtype.clone());
        }
    }
    newtypes
}

pub(super) fn completed_self_generated_source_with_facts(
    product: topaz_self_frontend::SelfCompilationProduct,
    target_facts: &topaz_self_frontend::SelfTargetAdapterFacts,
) -> Result<GeneratedSource, ExitCode> {
    let target_facts_json = self_target_adapter_facts_json(target_facts);
    let explicit_main = target_facts.has_explicit_main();
    let export_names = target_facts.entry_function_exports().collect::<Vec<_>>();
    let mut text = product.generated_rust().to_string();
    text.push_str(&self_product_rust_facade(
        explicit_main,
        &export_names,
        &target_facts_json,
    ));
    let compiler = self_compiler_provenance(&product, target_facts, &text);
    if compiler.target_compiler_fallback {
        eprintln!("topaz: self compilation product reported target compiler fallback");
        return Err(ExitCode::FAILURE);
    }
    Ok(GeneratedSource {
        text,
        compiler,
        lispex_application: None,
        explicit_main,
    })
}

pub(super) fn completed_self_generated_source(
    product: topaz_self_frontend::SelfCompilationProduct,
    label: &str,
    presentation: CheckPresentation,
) -> Result<GeneratedSource, ExitCode> {
    if product.status() != "completed" {
        let code = check_self_compilation_product(product, label, false, false, presentation);
        return Err(code);
    }
    let target_facts =
        topaz_self_frontend::project_target_adapter_facts(&product).map_err(|error| {
            eprintln!("topaz: self target facts are incomplete: {error}");
            ExitCode::FAILURE
        })?;
    completed_self_generated_source_with_facts(product, &target_facts)
}

pub(super) fn completed_self_python_source(
    product: topaz_self_frontend::SelfCompilationProduct,
    label: &str,
    presentation: CheckPresentation,
) -> Result<GeneratedSource, ExitCode> {
    if product.status() != "completed" {
        let code = check_self_compilation_product(product, label, false, false, presentation);
        return Err(code);
    }
    validate_self_python_operations(&product).map_err(|error| {
        eprintln!("topaz: self Python target declined before output: {error}");
        eprintln!("topaz: recovery: rerun with `--compiler rust` (not executed)");
        ExitCode::FAILURE
    })?;
    let runtime_inputs = topaz_self_frontend::project_self_target_runtime_inputs(&product)
        .map_err(|error| {
            eprintln!("topaz: self Python runtime inputs are incomplete: {error}");
            ExitCode::FAILURE
        })?;
    let target_facts_json = self_target_adapter_facts_json(&runtime_inputs.facts);
    let ir_json = runtime_inputs.ir_json;
    let adapter = format!(
        concat!(
            "\n\n# Generated by the Topaz Python target adapter. Do not edit.\n",
            "TOPAZ_COMPILER_IR_JSON = {}\n",
            "TOPAZ_TARGET_ADAPTER_FACTS_JSON = {}\n\n",
            "def run(stdin_text: str, args: list[str] | None = None) -> int:\n",
            "    return run_product(TOPAZ_COMPILER_IR_JSON, ",
            "TOPAZ_TARGET_ADAPTER_FACTS_JSON, stdin_text, args)\n\n",
            "if __name__ == \"__main__\":\n",
            "    import sys\n",
            "    raise SystemExit(run(sys.stdin.read(), args=sys.argv[1:]))\n",
        ),
        json_string(ir_json),
        json_string(&target_facts_json),
    );
    let mut text = String::with_capacity(topaz_emit_py::SELF_PRODUCT_RT.len() + adapter.len());
    text.push_str(topaz_emit_py::SELF_PRODUCT_RT);
    text.push_str(&adapter);
    let compiler = self_compiler_provenance(&product, &runtime_inputs.facts, &text);
    if compiler.target_compiler_fallback {
        eprintln!("topaz: self Python compilation reported target compiler fallback");
        return Err(ExitCode::FAILURE);
    }
    Ok(GeneratedSource {
        text,
        compiler,
        lispex_application: None,
        explicit_main: runtime_inputs.facts.has_explicit_main(),
    })
}

pub(super) fn validate_self_python_operations(
    product: &topaz_self_frontend::SelfCompilationProduct,
) -> Result<(), String> {
    const KINDS: &[&str] = &[
        "module",
        "export",
        "import",
        "record",
        "enum",
        "newtype",
        "type-alias",
        "function",
        "binding/capture",
        "binding/parameter",
        "pattern/binding",
        "pattern/typed-binding",
        "constant",
        "expression/block",
        "expression/integer",
        "expression/boolean",
        "expression/unit",
        "expression/string-text",
        "expression/string",
        "expression/identifier",
        "expression/array",
        "expression/member",
        "expression/call",
        "expression/record-literal",
        "expression/record-update",
        "expression/binary",
        "expression/unary",
        "expression/if",
        "expression/match",
        "expression/for",
        "expression/result-propagation",
        "let",
        "assignment",
        "return",
        "pattern/wildcard",
        "pattern/literal",
        "pattern/constructor",
    ];
    const METHODS: &[&str] = &[
        "allocate",
        "fromBytes",
        "get",
        "set",
        "fill",
        "copy",
        "toBytes",
        "toHex",
        "length",
    ];
    let functions = product
        .lowered()
        .operations
        .iter()
        .filter(|operation| operation.kind == "function" && !operation.binding_name.is_empty())
        .map(|operation| format!("{}::{}", operation.module, operation.binding_name))
        .collect::<BTreeSet<_>>();
    for operation in &product.lowered().operations {
        if !KINDS.contains(&operation.kind.as_str()) {
            return Err(format!(
                "operation `{}` has unsupported kind `{}`",
                operation.id, operation.kind
            ));
        }
        if operation.kind != "expression/call" {
            continue;
        }
        if matches!(
            operation.call_target.as_str(),
            "builtin::print"
                | "builtin::toInt"
                | "builtin::Some"
                | "builtin::None"
                | "builtin::Ok"
                | "builtin::Err"
        ) || functions.contains(&operation.call_target)
            || (!operation.call_method.is_empty()
                && METHODS.contains(&operation.call_method.as_str()))
        {
            continue;
        }
        return Err(format!(
            "call `{}` has unsupported target `{}` and method `{}`",
            operation.id, operation.call_target, operation.call_method
        ));
    }
    Ok(())
}

pub(super) fn json_string(value: &str) -> String {
    let mut result = String::with_capacity(value.len() + 2);
    result.push('"');
    for character in value.chars() {
        match character {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\u{08}' => result.push_str("\\b"),
            '\u{0c}' => result.push_str("\\f"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            character if character <= '\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(result, "\\u{:04x}", character as u32);
            }
            character => result.push(character),
        }
    }
    result.push('"');
    result
}

pub(super) fn self_target_adapter_facts_json(
    facts: &topaz_self_frontend::SelfTargetAdapterFacts,
) -> String {
    topaz_self_frontend::encode_target_adapter_facts(facts)
}

pub(super) fn self_product_rust_facade(
    explicit_main: bool,
    export_names: &[&str],
    target_facts_json: &str,
) -> String {
    let export_names = export_names
        .iter()
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"

// Mechanical target facade: the compiler already supplied checked fixed-point IR
// above. This layer only exposes the ordinary emitted-program host contract.
use std::rc::Rc;
use topaz_rt::{{
    CallFuture, DeadlineExceeded, FileId, Host, RunOutcome, Span, Value,
    canonical_abi_completed, canonical_abi_decode_args, canonical_abi_error,
    canonical_abi_faulted, codes, fault,
}};

pub const TOPAZ_EXPLICIT_MAIN: bool = {explicit_main};
pub const TOPAZ_TARGET_ADAPTER_FACTS_JSON: &str = {target_facts_json:?};

pub fn topaz_export_names() -> &'static [&'static str] {{
    &[{export_names}]
}}

pub fn run_with_host(host: Rc<dyn Host>) -> RunOutcome {{
    run_with_host_and_input(host, Vec::new(), String::new())
}}

pub fn run_with_host_and_input(
    host: Rc<dyn Host>,
    args: Vec<String>,
    stdin: String,
) -> RunOutcome {{
    match topaz_rt::execute_product_program_with_host_facts_and_input(
        TOPAZ_COMPILER_IR_JSON,
        &args,
        &stdin,
        Some(TOPAZ_TARGET_ADAPTER_FACTS_JSON),
        host,
    ) {{
        Ok((value, observed_main)) if observed_main == TOPAZ_EXPLICIT_MAIN => {{
            RunOutcome::Completed(value)
        }}
        Ok((_value, observed_main)) => RunOutcome::Faulted(fault(
            codes::GUARD_UNIMPLEMENTED,
            format!(
                "self product entry contract drifted: manifest main={{}}, runtime main={{observed_main}}",
                TOPAZ_EXPLICIT_MAIN,
            ),
            Span::new(FileId(0), 0, 0),
        )),
        Err(error) => RunOutcome::Faulted(fault(
            codes::GUARD_UNIMPLEMENTED,
            format!("self product runtime declined: {{error}}"),
            Span::new(FileId(0), 0, 0),
        )),
    }}
}}

fn self_product_fault(error: String) -> topaz_rt::RtError {{
    fault(
        codes::GUARD_UNIMPLEMENTED,
        format!("self product runtime declined: {{error}}"),
        Span::new(FileId(0), 0, 0),
    )
}}

fn self_product_export_allowed(name: &str) -> Result<(), String> {{
    if topaz_export_names().contains(&name) {{
        Ok(())
    }} else {{
        Err(format!("self product has no exported function `{{name}}`"))
    }}
}}

pub fn call_export_with_host(
    host: Rc<dyn Host>,
    name: &str,
    args: Vec<Value>,
) -> RunOutcome {{
    match self_product_export_allowed(name).and_then(|()| {{
        topaz_rt::execute_product_export_in_place_with_host_facts(
            TOPAZ_COMPILER_IR_JSON,
            name,
            args,
            Some(TOPAZ_TARGET_ADAPTER_FACTS_JSON),
            Some(host),
        )
    }}) {{
        Ok(value) => RunOutcome::Completed(value),
        Err(error) => RunOutcome::Faulted(self_product_fault(error)),
    }}
}}

pub fn call_export_with_host_until(
    host: Rc<dyn Host>,
    name: &str,
    args: Vec<Value>,
    deadline: std::time::Instant,
) -> Result<RunOutcome, DeadlineExceeded> {{
    let outcome = topaz_rt::block_on_until(
        deadline,
        call_export_with_host_future(host, name, args),
    )?;
    Ok(match outcome {{
        Ok(value) => RunOutcome::Completed(value),
        Err(error) => RunOutcome::Faulted(error),
    }})
}}

pub fn call_export_with_host_future(
    host: Rc<dyn Host>,
    name: &str,
    args: Vec<Value>,
) -> CallFuture {{
    let name = name.to_string();
    Box::pin(async move {{
        self_product_export_allowed(&name)
            .and_then(|()| {{
                topaz_rt::execute_product_export_in_place_with_host_facts(
                    TOPAZ_COMPILER_IR_JSON,
                    &name,
                    args,
                    Some(TOPAZ_TARGET_ADAPTER_FACTS_JSON),
                    Some(host),
                )
            }})
            .map_err(self_product_fault)
    }})
}}

pub fn call_export_with_host_and_input(
    host: Rc<dyn Host>,
    name: &str,
    args: Vec<Value>,
    _program_args: Vec<String>,
    _stdin: String,
) -> RunOutcome {{
    call_export_with_host(host, name, args)
}}

pub fn call_export_json_with_host(
    host: Rc<dyn Host>,
    name: &str,
    args_json: &str,
) -> String {{
    call_export_json_with_host_and_input(host, name, args_json, Vec::new(), String::new())
}}

pub fn call_export_json_with_host_and_input(
    host: Rc<dyn Host>,
    name: &str,
    args_json: &str,
    program_args: Vec<String>,
    stdin: String,
) -> String {{
    let args = match canonical_abi_decode_args(args_json) {{
        Ok(args) => args,
        Err(error) => return canonical_abi_error(&error),
    }};
    match call_export_with_host_and_input(host, name, args, program_args, stdin) {{
        RunOutcome::Completed(value) => canonical_abi_completed(&value),
        RunOutcome::Faulted(error) => canonical_abi_faulted(&error),
    }}
}}
"#
    )
}

pub(super) fn logical_entry(entry: &str) -> String {
    let normalized = entry.replace('\\', "/");
    let path = Path::new(&normalized);
    if path.is_absolute() {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "main.tpz".into())
    } else {
        normalized
    }
}

pub(super) fn sha256_identity(bytes: &[u8]) -> String {
    let digest = topaz_value::value::sha256(bytes);
    let mut value = String::from("sha256:");
    topaz_value::bytes_to_hex_into(&mut value, &digest);
    value
}

pub(super) fn resolved_target_source_set_id(
    unit: &topaz_resolve::ResolveOutput,
) -> Result<String, String> {
    if unit.modules.is_empty() {
        return Err("resolved product omitted every target module".to_string());
    }
    let mut identities = BTreeSet::new();
    let mut entry_count = 0usize;
    let mut material = Vec::new();
    for (ordinal, module) in unit.modules.iter().enumerate() {
        if !identities.insert(module.identity.as_str()) {
            return Err(format!(
                "resolved product repeats module identity `{}`",
                module.identity
            ));
        }
        entry_count += usize::from(module.is_entry);
        for value in [
            ordinal.to_string(),
            module.identity.clone(),
            module.path.clone(),
            module.is_entry.to_string(),
            module.is_extern.to_string(),
            sha256_identity(unit.map.file(module.file).src().as_bytes()),
        ] {
            material.extend_from_slice(value.len().to_string().as_bytes());
            material.push(b':');
            material.extend_from_slice(value.as_bytes());
            material.push(0);
        }
    }
    if entry_count != 1 {
        return Err(format!(
            "resolved product requires one entry module, observed {entry_count}"
        ));
    }
    Ok(sha256_identity(&material))
}

pub(super) fn rust_compiler_provenance(
    unit: &topaz_resolve::ResolveOutput,
    generated_source: &str,
) -> Result<artifact::CompilerProvenance, String> {
    let compiler_source_set_id = topaz_kernel::compiler_source_set_id().to_string();
    let target_source_set_id = resolved_target_source_set_id(unit)?;
    let generated_source_sha256 = sha256_identity(generated_source.as_bytes());
    let compile_product_id = sha256_identity(
        format!(
            "rust\0rust-stage0\0{compiler_source_set_id}\0{target_source_set_id}\0{generated_source_sha256}"
        )
        .as_bytes(),
    );
    Ok(artifact::CompilerProvenance {
        selector: "rust".to_string(),
        producer: "rust-stage0".to_string(),
        selection_origin: invocation_selection_origin().label().to_string(),
        compiler_source_set_id,
        target_source_set_id,
        compile_product_id,
        generated_source_sha256,
        target_compiler_fallback: false,
    })
}

pub(super) fn self_compiler_provenance(
    product: &topaz_self_frontend::SelfCompilationProduct,
    target_facts: &topaz_self_frontend::SelfTargetAdapterFacts,
    generated_source: &str,
) -> artifact::CompilerProvenance {
    artifact::CompilerProvenance {
        selector: "self".to_string(),
        producer: target_facts.producer.to_string(),
        selection_origin: invocation_selection_origin().label().to_string(),
        compiler_source_set_id: product.compiler().source_set_id.clone(),
        target_source_set_id: target_facts.source_set_id.clone(),
        compile_product_id: target_facts.result_id.clone(),
        generated_source_sha256: sha256_identity(generated_source.as_bytes()),
        target_compiler_fallback: false,
    }
}

pub(super) fn explicit_main_exit(value: Value, explicit_main: bool) -> ExitCode {
    if !explicit_main {
        return ExitCode::SUCCESS;
    }
    match value {
        Value::Ok(inner) => match inner.as_ref() {
            Value::Int(code) if (0..=255).contains(code) => ExitCode::from(*code as u8),
            Value::Int(code) => {
                eprintln!("topaz: explicit main returned exit code {code}; expected 0..255");
                ExitCode::FAILURE
            }
            other => {
                eprintln!(
                    "topaz: explicit main returned `Ok({})`; expected `Ok(int)`",
                    other.kind()
                );
                ExitCode::FAILURE
            }
        },
        Value::Err(inner) => match inner.as_ref() {
            Value::Str(message) => {
                eprintln!("{message}");
                ExitCode::FAILURE
            }
            other => {
                eprintln!(
                    "topaz: explicit main returned `Err({})`; expected `Err(string)`",
                    other.kind()
                );
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!(
                "topaz: explicit main returned `{}`; expected `Result<int, string>`",
                other.kind()
            );
            ExitCode::FAILURE
        }
    }
}

pub(super) fn program_args_are_admitted(explicit_main: bool, program_args: &[String]) -> bool {
    program_args.is_empty() || explicit_main
}

pub(super) fn admit_cli_program_args(
    explicit_main: bool,
    program_args: &[String],
) -> Result<(), ExitCode> {
    if program_args_are_admitted(explicit_main, program_args) {
        return Ok(());
    }
    eprintln!("topaz: `--` program args require an exported `main(args, stdin)`");
    Err(ExitCode::FAILURE)
}

/// Resolve a v5.2 single-module unit and lower it to Rust via `emit_module` — the
/// shared front half of `emit` and `build`. By default the resolved unit is
/// statically type-checked (CDR-003 §13) before lowering — the same gate as `run`
/// and `check`; `--unchecked` (`unchecked == true`) skips that check and lowers on
/// the resolution-only, runtime-semantics path the differential harness pins. v5.1
/// has no module system, so it is rejected (like `check`). On any failure the
/// diagnostics / message are printed and the caller's `ExitCode::FAILURE` is
/// returned. `cmd` names the command for the v5.1 message.
pub(super) struct RustLoweringRequest<'request> {
    pub(super) entry: &'request str,
    pub(super) root: Option<&'request str>,
    pub(super) version: LangVersion,
    pub(super) command: &'request str,
    pub(super) unchecked: bool,
    pub(super) backend: Backend,
    pub(super) native_report: Option<&'request mut NativeReportSession>,
    pub(super) build_target: &'request str,
}

pub(super) fn resolve_and_lower(
    request: RustLoweringRequest<'_>,
) -> Result<GeneratedSource, ExitCode> {
    let RustLoweringRequest {
        entry,
        root,
        version,
        command: cmd,
        unchecked,
        backend,
        native_report,
        build_target,
    } = request;
    if version == LangVersion::V5_1 {
        eprintln!(
            "topaz: `{cmd}` needs v5.2+ (v5.16 is the default); `--language-version 5.1` has no module system"
        );
        return Err(ExitCode::FAILURE);
    }
    let entry_norm = entry.replace('\\', "/");
    let (base, entry_rel, root_rel) = split_absolute(&entry_norm, root).map_err(|msg| {
        eprintln!("topaz: {msg}");
        ExitCode::FAILURE
    })?;
    let provider = PhysicalProvider::new(base);
    let unit = resolve_with_version(&provider, &entry_rel, root_rel.as_deref(), version);
    for diag in &unit.diagnostics {
        eprintln!("{}", render(diag, &unit.map));
    }
    if has_errors(&unit.diagnostics) {
        return Err(ExitCode::FAILURE);
    }
    // CDR-003 §13: `emit`/`build` statically type-check by default — the same gate
    // as `topaz check` and `run`, reported identically — so the three execution
    // paths admit exactly the same programs at the CLI surface. `--unchecked` opts
    // out for the runtime-semantics workflow. (Independently of this gate, the
    // emitter may still refuse a *well-typed* program it cannot lower yet, via
    // `EmitError::Unsupported` below; that is a capability limit, not a type gate.)
    //
    // The interpreter/emitter differential harness (CDR-006 §7) does NOT take this
    // path: it lowers via the internal `emit_module` API with no checker in the
    // loop, so run≡build equivalence stays pinned over the curated runtime-
    // semantics corpus regardless of this default.
    let checked = if unchecked {
        None
    } else {
        match check_resolved_unit(&unit, false, version) {
            Ok(checked) => Some(checked),
            Err(n) => {
                eprintln!(
                    "{entry_norm}: {n} type diagnostic{}",
                    if n == 1 { "" } else { "s" }
                );
                return Err(ExitCode::FAILURE);
            }
        }
    };
    let lowered = lower_rust_input(&unit, checked.as_ref())?;
    // v5.4 native backend (checked builds opting in via `--backend native`): try
    // the monomorphized native lowering first, FALLING BACK to boxed on a
    // structured native DECLINE (TPZ6002) — never on a boxed coverage gap or a
    // hard error. The native backend consumes the typed HIR a clean check
    // produces (the gate above already ran), so this is sound by construction.
    if backend == Backend::Native {
        let native_input = topaz_emit::NativeInput { unit: &lowered };
        if let Ok(outcome) = topaz_emit::emit_native_or_hybrid(&native_input) {
            if let Some(report) = native_report {
                report.capture(
                    if cmd == "emit" { "emit" } else { "build" },
                    build_target,
                    version,
                    outcome.decision,
                );
            }
            let compiler = rust_compiler_provenance(&unit, &outcome.rust).map_err(|error| {
                eprintln!("topaz: cannot record compiler provenance: {error}");
                ExitCode::FAILURE
            })?;
            return Ok(GeneratedSource {
                text: outcome.rust,
                compiler,
                lispex_application: None,
                explicit_main: topaz_resolve::has_explicit_main(&unit),
            });
        }
    }
    let text = topaz_emit::emit_module(&lowered).map_err(|e| {
        // The emitter OWNS the diagnostic (TPZ6001 code, located span, remedy note);
        // the CLI just renders it like every other diagnostic. An internal defect
        // (`NoEntry`) or an unlocated error has no diagnostic — fall back to a plain
        // line so `emit`/`build` still fail loudly.
        match e.diagnostic() {
            Some(diag) => eprintln!("{}", render(&diag, &unit.map)),
            None => eprintln!("topaz: cannot compile this program yet — {e}"),
        }
        ExitCode::FAILURE
    })?;
    let compiler = rust_compiler_provenance(&unit, &text).map_err(|error| {
        eprintln!("topaz: cannot record compiler provenance: {error}");
        ExitCode::FAILURE
    })?;
    Ok(GeneratedSource {
        text,
        compiler,
        lispex_application: None,
        explicit_main: topaz_resolve::has_explicit_main(&unit),
    })
}

pub(super) fn python_banner(command: &str) -> String {
    format!(
        "# Generated Topaz Python application artifact.\n\
# Replaceable compiler output; edit the .tpz source and regenerate.\n\
# Generated by `topaz {command} --target python`.\n"
    )
}

pub(super) fn python_program(generated: &str, command: &str) -> String {
    let mut out = String::with_capacity(generated.len() + 512);
    out.push_str(&python_banner(command));
    let mut rest = generated;
    if let Some(index) = rest.find("from __future__ import ") {
        out.push_str(&rest[..index]);
        rest = &rest[index..];
        if let Some((line, tail)) = rest.split_once('\n') {
            out.push_str(line);
            out.push('\n');
            rest = tail;
        }
    }
    rest = rest.strip_prefix('\n').unwrap_or(rest);
    out.push_str("\nimport sys as _topaz_sys\n");
    out.push_str("_topaz_previous_dont_write_bytecode = _topaz_sys.dont_write_bytecode\n");
    out.push_str("_topaz_sys.dont_write_bytecode = True\n\n");
    if let Some(import_end) = rest.find("\nIR_SCHEMA = ") {
        out.push_str(&rest[..=import_end]);
        out.push_str("_topaz_sys.dont_write_bytecode = _topaz_previous_dont_write_bytecode\n\n");
        out.push_str(&rest[import_end + 1..]);
    } else if let Some((imports, rest)) = rest.split_once("\n\n") {
        out.push_str(imports);
        out.push_str("\n_topaz_sys.dont_write_bytecode = _topaz_previous_dont_write_bytecode\n\n");
        out.push_str(rest);
    } else {
        out.push_str(rest);
        out.push_str("\n_topaz_sys.dont_write_bytecode = _topaz_previous_dont_write_bytecode\n");
    }
    out
}

pub(super) fn resolve_and_emit_python_entry(
    entry: &str,
    root: Option<&str>,
    version: LangVersion,
    unchecked: bool,
) -> Result<GeneratedSource, ExitCode> {
    if version == LangVersion::V5_1 {
        eprintln!(
            "topaz: the Python target needs v5.2+ (v5.16 is the default); `--language-version 5.1` has no module system"
        );
        return Err(ExitCode::FAILURE);
    }
    let entry_norm = entry.replace('\\', "/");
    let (base, entry_rel, root_rel) = split_absolute(&entry_norm, root).map_err(|msg| {
        eprintln!("topaz: {msg}");
        ExitCode::FAILURE
    })?;
    let provider = PhysicalProvider::new(base);
    let unit = resolve_with_version(&provider, &entry_rel, root_rel.as_deref(), version);
    lower_python_resolved_unit(
        &unit,
        PythonLoweringContext {
            version,
            unchecked,
            label: &entry_norm,
            extern_replay_jsonl: None,
            extern_sandbox_policies: &[],
            application_fs_roots: None,
            package_target: None,
        },
    )
}

pub(super) fn resolve_and_emit_python_package(
    target: &PackageTarget,
    unchecked: bool,
) -> Result<GeneratedSource, ExitCode> {
    let unit = resolve_package_target(target);
    lower_python_resolved_unit(
        &unit,
        PythonLoweringContext {
            version: target.version,
            unchecked,
            label: &target.entry,
            extern_replay_jsonl: Some(&target.extern_replay_jsonl),
            extern_sandbox_policies: &target.extern_sandbox_policies,
            application_fs_roots: None,
            package_target: Some(target),
        },
    )
}

pub(super) fn resolve_and_emit_python_application_package(
    target: &PackageTarget,
    unchecked: bool,
) -> Result<GeneratedSource, ExitCode> {
    let unit = resolve_package_target(target);
    lower_python_resolved_unit(
        &unit,
        PythonLoweringContext {
            version: target.version,
            unchecked,
            label: &target.entry,
            extern_replay_jsonl: Some(&target.extern_replay_jsonl),
            extern_sandbox_policies: &target.extern_sandbox_policies,
            application_fs_roots: Some((&target.fs_read_roots, &target.fs_write_roots)),
            package_target: Some(target),
        },
    )
}

pub(super) struct PythonLoweringContext<'context> {
    pub(super) version: LangVersion,
    pub(super) unchecked: bool,
    pub(super) label: &'context str,
    pub(super) extern_replay_jsonl: Option<&'context str>,
    pub(super) extern_sandbox_policies: &'context [topaz_value::ExternSandboxPolicy],
    pub(super) application_fs_roots: Option<(&'context [String], &'context [String])>,
    pub(super) package_target: Option<&'context PackageTarget>,
}

pub(super) fn lower_python_resolved_unit(
    unit: &topaz_resolve::ResolveOutput,
    context: PythonLoweringContext<'_>,
) -> Result<GeneratedSource, ExitCode> {
    let PythonLoweringContext {
        version,
        unchecked,
        label,
        extern_replay_jsonl,
        extern_sandbox_policies,
        application_fs_roots,
        package_target,
    } = context;
    for diag in &unit.diagnostics {
        eprintln!("{}", render(diag, &unit.map));
    }
    if has_errors(&unit.diagnostics) {
        return Err(ExitCode::FAILURE);
    }
    // Python parity follows native run/emit semantics: --unchecked is an
    // explicit parity-debug opt-out, unlike web's typed-facade contract.
    let checked = if unchecked {
        None
    } else {
        match check_resolved_unit(unit, false, version) {
            Ok(checked) => Some(checked),
            Err(n) => {
                eprintln!(
                    "{label}: {n} type diagnostic{}",
                    if n == 1 { "" } else { "s" }
                );
                return Err(ExitCode::FAILURE);
            }
        }
    };
    if let Some(target) = package_target {
        match checked.as_ref() {
            Some(checked) => {
                reject_reached_lispex_application_target(target, checked, "python")?;
            }
            None if !target.generated_std_modules.is_empty() => {
                eprintln!(
                    "topaz: the Lispex application surface requires checked reachability; drop `--unchecked`"
                );
                return Err(ExitCode::FAILURE);
            }
            None => {}
        }
    }
    let checked_aliases = checked.as_ref().map(|checked| &checked.local_aliases);
    let text = match application_fs_roots {
        Some((read_roots, write_roots)) => {
            topaz_emit_py::emit_application_module_with_checked_aliases_and_extern_replay_and_policies(
                unit,
                checked_aliases,
                extern_replay_jsonl,
                extern_sandbox_policies,
                read_roots,
                write_roots,
            )
        }
        None => topaz_emit_py::emit_module_with_checked_aliases_and_extern_replay_and_policies(
            unit,
            checked_aliases,
            extern_replay_jsonl,
            extern_sandbox_policies,
        ),
    }
    .map_err(|e| {
        eprintln!("topaz: Python target cannot emit this program — {e}");
        if let Some(span) = e.span {
            eprintln!(
                "topaz: Python target decline span: file {}, bytes {}..{}",
                span.file.0, span.lo, span.hi
            );
        }
        ExitCode::FAILURE
    })?;
    let compiler = rust_compiler_provenance(unit, &text).map_err(|error| {
        eprintln!("topaz: cannot record compiler provenance: {error}");
        ExitCode::FAILURE
    })?;
    Ok(GeneratedSource {
        text,
        compiler,
        lispex_application: None,
        explicit_main: topaz_resolve::has_explicit_main(unit),
    })
}

pub(super) fn write_python_artifact(
    dir: &str,
    generated: &str,
    command: &str,
) -> Result<(), String> {
    let out_dir = Path::new(dir);
    fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let program = python_program(generated, command);
    fs::write(out_dir.join("program.py"), program).map_err(|e| e.to_string())?;
    fs::write(out_dir.join("topaz_py_rt.py"), topaz_emit_py::PY_RT).map_err(|e| e.to_string())?;
    fs::write(out_dir.join("LICENSE-RUNTIME"), artifact::license_text())
        .map_err(|e| e.to_string())?;
    fs::write(out_dir.join("NOTICE"), artifact::notice_text()).map_err(|e| e.to_string())?;
    fs::write(
        out_dir.join(artifact::OUTPUT_NOTICE_NAME),
        artifact::output_notice_text(),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn write_self_python_artifact(
    dir: &str,
    generated: &str,
    command: &str,
) -> Result<(), String> {
    write_python_artifact(dir, generated, command)?;
    Ok(())
}

pub(super) fn emit_python_source(out_dir: Option<&str>, generated: &str) -> ExitCode {
    match out_dir {
        None => {
            print!("{}", python_program(generated, "emit"));
            ExitCode::SUCCESS
        }
        Some("") => {
            eprintln!("topaz: `--out-dir` requires a non-empty directory");
            ExitCode::FAILURE
        }
        Some(dir) => match write_python_artifact(dir, generated, "emit") {
            Ok(()) => {
                eprintln!(
                    "topaz: wrote Python source set to `{}` (`program.py` + `topaz_py_rt.py`)",
                    Path::new(dir).display()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("topaz: could not write the Python source set to `{dir}`: {e}");
                ExitCode::FAILURE
            }
        },
    }
}

pub(super) fn emit_self_python_source(out_dir: Option<&str>, generated: &str) -> ExitCode {
    match out_dir {
        None => {
            print!("{}", python_program(generated, "emit"));
            ExitCode::SUCCESS
        }
        Some("") => {
            eprintln!("topaz: `--out-dir` requires a non-empty directory");
            ExitCode::FAILURE
        }
        Some(dir) => match write_self_python_artifact(dir, generated, "emit") {
            Ok(()) => {
                eprintln!(
                    "topaz: wrote self Python source set to `{}` (`program.py` + target runtimes)",
                    Path::new(dir).display()
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("topaz: could not write the self Python source set to `{dir}`: {error}");
                ExitCode::FAILURE
            }
        },
    }
}

/// `topaz emit <entry> [--out-dir <dir>]` (CDR-006 E-3): lower the program to Rust.
/// Without `--out-dir` it prints to stdout; with it, a complete, `cargo run`-able
/// crate is scaffolded into the directory.
pub(super) struct EmitEntryRequest<'request> {
    pub(super) entry: &'request str,
    pub(super) root: Option<&'request str>,
    pub(super) out_dir: Option<&'request str>,
    pub(super) standalone_version: LangVersion,
    pub(super) unchecked_flag: bool,
    pub(super) backend: Backend,
    pub(super) emit_target: EmitTarget,
    pub(super) native_report: Option<&'request mut NativeReportSession>,
}

pub(super) fn emit_entry(request: EmitEntryRequest<'_>) -> ExitCode {
    let EmitEntryRequest {
        entry,
        root,
        out_dir,
        standalone_version: version,
        unchecked_flag: unchecked,
        backend,
        emit_target,
        native_report,
    } = request;
    if emit_target.is_python() {
        let generated = match resolve_and_emit_python_entry(entry, root, version, unchecked) {
            Ok(generated) => generated,
            Err(code) => return code,
        };
        return emit_python_source(out_dir, &generated.text);
    }
    let rust = match resolve_and_lower(RustLoweringRequest {
        entry,
        root,
        version,
        command: "emit",
        unchecked,
        backend,
        native_report,
        build_target: "rust",
    }) {
        Ok(rust) => rust,
        Err(code) => return code,
    };
    match out_dir {
        None => {
            print!("{}", rust.text);
            ExitCode::SUCCESS
        }
        Some("") => {
            eprintln!("topaz: `--out-dir` requires a non-empty directory");
            ExitCode::FAILURE
        }
        Some(dir) => {
            if let Err(e) = scaffold_crate(Path::new(dir), &rust.text, HostHarness::Unrestricted) {
                eprintln!("topaz: could not write the crate to `{dir}`: {e}");
                return ExitCode::FAILURE;
            }
            let env = match prepare_build_env(Path::new(dir)) {
                Ok(env) => env,
                Err(code) => return code,
            };
            let locked = generate_lockfile(&env);
            env.cleanup();
            if let Err(code) = locked {
                return code;
            }
            eprintln!(
                "topaz: wrote a self-contained crate to `{dir}` (vendored runtime + Cargo.lock; \
                 build with `cd {dir} && cargo build --offline --locked`)"
            );
            ExitCode::SUCCESS
        }
    }
}

/// `topaz build <entry> --out-dir <dir>` (CDR-006 E-3): lower the program, scaffold
/// the crate into `<dir>`, then drive `cargo build` on it to produce a native
/// binary at `<dir>/target/debug/program`. `--out-dir` is required (the build needs
/// somewhere to put the crate + target). cargo's own output streams through; a
/// resolution / emit / scaffold / cargo failure each fails cleanly.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_entry(
    entry: &str,
    root: Option<&str>,
    out_dir: Option<&str>,
    release: bool,
    run: bool,
    version: LangVersion,
    unchecked: bool,
    backend: Backend,
    build_target: BuildTarget,
    experimental: bool,
    program_args: &[String],
    mut native_report: Option<&mut NativeReportSession>,
) -> ExitCode {
    let build_target = if build_target == BuildTarget::Default {
        BuildTarget::Native
    } else {
        build_target
    };
    let dir = match out_dir {
        Some("") | None => {
            eprintln!("topaz: `build` requires `--out-dir <dir>` (a non-empty directory)");
            return ExitCode::FAILURE;
        }
        Some(dir) => dir,
    };
    if experimental {
        eprintln!(
            "topaz: warning: `--experimental` is deprecated; Python is a regular deployment target in v5.9"
        );
    }
    if build_target.is_service() {
        eprintln!(
            "topaz: `build --target http-service` is package-only; run it from a package root without an entry argument"
        );
        return ExitCode::FAILURE;
    }
    if build_target.is_python() {
        let destination =
            match artifact::Destination::open(Path::new(dir), artifact::Target::Python) {
                Ok(destination) => destination,
                Err(e) => {
                    eprintln!("topaz: cannot use output directory: {e}");
                    return ExitCode::FAILURE;
                }
            };
        let generated = match resolve_and_emit_python_entry(entry, root, version, unchecked) {
            Ok(generated) => generated,
            Err(code) => return code,
        };
        return install_python_build(
            destination,
            Path::new(dir),
            entry,
            version,
            &generated.text,
            generated.compiler,
        );
    }
    if build_target.is_web() {
        let lowered = match resolve_and_lower_entry_for_web(
            entry,
            root,
            version,
            backend,
            native_report.as_deref_mut(),
            "build",
            build_target.label(),
        ) {
            Ok(lowered) => lowered,
            Err(code) => return code,
        };
        if build_target == BuildTarget::WebApp {
            eprintln!(
                "topaz: `build --target web-app` is package-only; run it from a package root without an entry argument"
            );
            return ExitCode::FAILURE;
        }
        return build_web_package(WebPackageBuild {
            dir: Path::new(dir),
            rust: &lowered.rust,
            compiler: lowered.compiler,
            release,
            label: entry,
            entry_exports: &lowered.entry_exports,
            records: &lowered.records,
            enums: &lowered.enums,
            newtypes: &lowered.newtypes,
            language_version: version,
            target: build_target,
            package_root: None,
            package_name: None,
            web: None,
            web_capabilities: None,
        });
    }
    let rust = match resolve_and_lower(RustLoweringRequest {
        entry,
        root,
        version,
        command: "build",
        unchecked,
        backend,
        native_report,
        build_target: build_target.label(),
    }) {
        Ok(rust) => rust,
        Err(code) => return code,
    };
    build_native_artifact(
        Path::new(dir),
        entry,
        version,
        &rust.text,
        rust.compiler,
        HostHarness::Unrestricted,
        rust.explicit_main,
        release,
        run,
        program_args,
    )
}
