use crate::*;

pub(super) fn self_preview_span(
    modules: &[topaz_kernel::CanonicalPreviewModule],
    module_index: usize,
    lo: u32,
    hi: u32,
) -> Result<Span, String> {
    let module = modules.get(module_index).ok_or_else(|| {
        format!("self compiler diagnostic references missing module {module_index}")
    })?;
    if lo > hi
        || hi as usize > module.source.len()
        || !module.source.is_char_boundary(lo as usize)
        || !module.source.is_char_boundary(hi as usize)
    {
        return Err(format!(
            "self compiler diagnostic carries invalid span {lo}..{hi} for `{}`",
            module.path
        ));
    }
    let file = u32::try_from(module_index)
        .map_err(|_| "self compiler diagnostic module index exceeds u32".to_string())?;
    Ok(Span::new(FileId(file), lo, hi))
}

pub(super) fn self_preview_source_map(
    modules: &[topaz_kernel::CanonicalPreviewModule],
) -> Result<SourceMap, String> {
    let mut map = SourceMap::new();
    for (index, module) in modules.iter().enumerate() {
        let file = map
            .add_file(&module.path, module.source.clone())
            .map_err(|error| {
                format!("cannot map self compiler source `{}`: {error}", module.path)
            })?;
        if file.0 as usize != index {
            return Err("self compiler source-map order drifted".to_string());
        }
    }
    Ok(map)
}

pub(super) fn self_resolver_diagnostic(
    diagnostic: &topaz_kernel::CanonicalPreviewResolvedDiagnostic,
    modules: &[topaz_kernel::CanonicalPreviewModule],
) -> Result<Diagnostic, String> {
    let span = self_preview_span(
        modules,
        diagnostic.module_index,
        diagnostic.lo,
        diagnostic.hi,
    )?;
    Ok(Diagnostic::error(
        Code::new(SELF_DIAGNOSTIC_CODE_PLACEHOLDER),
        diagnostic.message.clone(),
        Label::new(span, ""),
    ))
}

pub(super) fn self_checker_diagnostic(
    diagnostic: &topaz_kernel::CanonicalPreviewCheckDiagnostic,
    modules: &[topaz_kernel::CanonicalPreviewModule],
) -> Result<Diagnostic, String> {
    let span = self_preview_span(
        modules,
        diagnostic.module_index,
        diagnostic.lo,
        diagnostic.hi,
    )?;
    let mut rendered = Diagnostic::error(
        Code::new(SELF_DIAGNOSTIC_CODE_PLACEHOLDER),
        diagnostic.message.clone(),
        Label::new(span, diagnostic.primary_message.clone()),
    );
    for secondary in &diagnostic.secondary {
        rendered = rendered.with_secondary(
            self_preview_span(modules, secondary.module_index, secondary.lo, secondary.hi)?,
            secondary.message.clone(),
        );
    }
    for note in &diagnostic.notes {
        rendered = rendered.with_note(note.clone());
    }
    Ok(rendered)
}

pub(super) fn render_self_diagnostic(
    diagnostic: &Diagnostic,
    code: &str,
    map: &SourceMap,
    json: bool,
) -> String {
    let rendered = if json {
        render_json(diagnostic, map)
    } else {
        render(diagnostic, map)
    };
    rendered.replacen(SELF_DIAGNOSTIC_CODE_PLACEHOLDER, code, 1)
}

pub(super) fn render_self_profile_diagnostic(
    diagnostic: &Diagnostic,
    code: &str,
    map: &SourceMap,
    profile_name: &str,
    profile_rule: Option<&str>,
) -> String {
    let mut out = String::from("{\"schema\":\"topaz.profile-diagnostic/v1\",\"profile\":");
    push_json_string(&mut out, profile_name);
    out.push_str(",\"rule\":");
    match profile_rule {
        Some(rule) => push_json_string(&mut out, rule),
        None => out.push_str("null"),
    }
    out.push_str(",\"diagnostic\":");
    out.push_str(&render_self_diagnostic(diagnostic, code, map, true));
    out.push_str(",\"fix\":");
    if let Some(replacement) = lsp_diagnostic_replacement(diagnostic) {
        let span = diagnostic.primary.span;
        let file = map.file(span.file);
        let start = file.line_col(span.lo);
        let end = file.line_col(span.hi);
        out.push_str("{\"applicability\":\"machine-applicable\",\"description\":");
        push_json_string(&mut out, &format!("Replace with `{replacement}`"));
        out.push_str(",\"edit\":{\"file\":");
        push_json_string(&mut out, file.name());
        out.push_str(&format!(
            ",\"line\":{},\"col\":{},\"endLine\":{},\"endCol\":{},\"lo\":{},\"hi\":{},\"replacement\":",
            start.line, start.col, end.line, end.col, span.lo, span.hi
        ));
        push_json_string(&mut out, &replacement);
        out.push_str("}}");
    } else {
        out.push_str("null");
    }
    out.push('}');
    out
}

pub(super) fn check_self_compilation_product(
    product: topaz_self_frontend::SelfCompilationProduct,
    label: &str,
    json: bool,
    exports_json: bool,
    presentation: CheckPresentation,
) -> ExitCode {
    if let Err(error) = topaz_self_frontend::encode_self_compilation_product_manifest(&product) {
        eprintln!("topaz: self compilation product is invalid: {error}");
        return ExitCode::FAILURE;
    }
    let preview = product.typed();
    let modules = &preview.resolved.modules;
    let map = match self_preview_source_map(modules) {
        Ok(map) => map,
        Err(error) => {
            eprintln!("topaz: self compilation product is invalid: {error}");
            return ExitCode::FAILURE;
        }
    };
    let resolver_diagnostics = match preview
        .resolved
        .diagnostics
        .iter()
        .map(|diagnostic| self_resolver_diagnostic(diagnostic, modules))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(diagnostics) => diagnostics,
        Err(error) => {
            eprintln!("topaz: self compilation product is invalid: {error}");
            return ExitCode::FAILURE;
        }
    };
    let checker_diagnostics = match preview
        .diagnostics
        .iter()
        .map(|diagnostic| self_checker_diagnostic(diagnostic, modules))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(diagnostics) => diagnostics,
        Err(error) => {
            eprintln!("topaz: self compilation product is invalid: {error}");
            return ExitCode::FAILURE;
        }
    };
    let profile_name = product.profile().identity();
    for (source, diagnostic) in preview
        .resolved
        .diagnostics
        .iter()
        .zip(&resolver_diagnostics)
    {
        let rendered = if profile_name.is_empty() {
            render_self_diagnostic(diagnostic, &source.code, &map, json)
        } else if json {
            render_self_profile_diagnostic(diagnostic, &source.code, &map, profile_name, None)
        } else {
            format!(
                "profile[{profile_name}]\n{}",
                render_self_diagnostic(diagnostic, &source.code, &map, false)
            )
        };
        eprintln!("{rendered}");
    }
    for (source, diagnostic) in preview.diagnostics.iter().zip(&checker_diagnostics) {
        let rendered = if profile_name.is_empty() {
            render_self_diagnostic(diagnostic, &source.code, &map, json)
        } else if json {
            render_self_profile_diagnostic(
                diagnostic,
                &source.code,
                &map,
                profile_name,
                source.profile_rule.as_deref(),
            )
        } else {
            format!(
                "profile[{profile_name}]\n{}",
                render_self_diagnostic(diagnostic, &source.code, &map, false)
            )
        };
        eprintln!("{rendered}");
    }
    let diagnostic_count = resolver_diagnostics.len() + checker_diagnostics.len();
    if !profile_name.is_empty() && json {
        println!(
            "{{\"schema\":\"topaz.profile-check/v1\",\"profile\":\"{profile_name}\",\"language\":\"topaz-{}\",\"status\":\"{}\",\"diagnosticCount\":{diagnostic_count},\"errorCount\":{diagnostic_count}}}",
            LangVersion::CURRENT.as_str(),
            if diagnostic_count == 0 {
                "pass"
            } else {
                "fail"
            },
        );
    }
    if diagnostic_count > 0 {
        if !json {
            if !profile_name.is_empty() {
                eprintln!(
                    "profile[{profile_name}] {label}: {diagnostic_count} diagnostic{}",
                    if diagnostic_count == 1 { "" } else { "s" }
                );
            } else if !resolver_diagnostics.is_empty()
                && matches!(presentation, CheckPresentation::Standalone)
            {
                eprintln!(
                    "{label}: {} diagnostic{}",
                    resolver_diagnostics.len(),
                    if resolver_diagnostics.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                );
            } else {
                eprintln!(
                    "{label}: {} type diagnostic{}",
                    checker_diagnostics.len(),
                    if checker_diagnostics.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                );
            }
        }
        return ExitCode::FAILURE;
    }
    if exports_json {
        println!("{}", render_self_export_surface_json(&product));
    } else if !json
        && (!profile_name.is_empty() || matches!(presentation, CheckPresentation::Standalone))
    {
        let prefix = if profile_name.is_empty() {
            String::new()
        } else {
            format!("profile[{profile_name}] ")
        };
        println!(
            "{prefix}{label}: types-ok ({} module{})",
            modules.len(),
            if modules.len() == 1 { "" } else { "s" }
        );
        println!(
            "{prefix}{label}: resolve-ok ({} module{})",
            modules.len(),
            if modules.len() == 1 { "" } else { "s" }
        );
    }
    ExitCode::SUCCESS
}

pub(super) fn render_self_export_surface_json(
    product: &topaz_self_frontend::SelfCompilationProduct,
) -> String {
    let mut modules = product
        .typed()
        .resolved
        .modules
        .iter()
        .map(|module| (module.identity.as_str(), Vec::new()))
        .collect::<BTreeMap<&str, Vec<&topaz_kernel::CanonicalPreviewResolvedExport>>>();
    for export in &product.typed().resolved.exports {
        if let Some(module) = product.typed().resolved.modules.get(export.module_index) {
            modules.entry(&module.identity).or_default().push(export);
        }
    }
    let mut out = String::from("{\"modules\":[");
    for (module_index, (identity, mut exports)) in modules.into_iter().enumerate() {
        if module_index > 0 {
            out.push(',');
        }
        exports.sort_by_key(|export| (&export.namespace, &export.name));
        let mut body = String::new();
        body.push_str("{\"identity\":");
        push_json_string(&mut body, identity);
        body.push_str(",\"ambient\":false,\"values\":[");
        let values = exports
            .iter()
            .filter(|export| export.namespace == "value")
            .copied()
            .collect::<Vec<_>>();
        for (index, export) in values.iter().enumerate() {
            if index > 0 {
                body.push(',');
            }
            let semantic_type = product
                .typed()
                .exported_value_node(export)
                .map(|node| render_semantic_type(&node.ty))
                .unwrap_or_else(|| "unknown".to_string());
            let declaration_index = self_ast_declaration_index(
                product
                    .typed()
                    .resolved
                    .modules
                    .get(export.module_index)
                    .expect("self export module was collected above"),
                export.declaration_lo,
                export.declaration_hi,
            );
            let module = product
                .typed()
                .resolved
                .modules
                .get(export.module_index)
                .expect("self export module was collected above");
            let function_shape = declaration_index.and_then(|declaration_index| {
                (module.ast[declaration_index].kind == "function-declaration").then(|| {
                    let type_parameters =
                        self_ast_children(module, declaration_index, "typeParameters").len();
                    let mut bounds = vec![Vec::new(); type_parameters];
                    for (_, bound) in
                        self_ast_children(module, declaration_index, "typeParameterBounds")
                    {
                        let parameter_index = (bound.index >> 32) as usize;
                        if let Some(parameter_bounds) = bounds.get_mut(parameter_index) {
                            parameter_bounds.push(
                                self_ast_text(module, bound)
                                    .expect("validated self function bound")
                                    .to_string(),
                            );
                        }
                    }
                    let parameters = self_ast_children(module, declaration_index, "parameters");
                    let mut names = Vec::with_capacity(parameters.len());
                    let mut defaulted = Vec::with_capacity(parameters.len());
                    for (parameter_index, _) in parameters {
                        let name = self_ast_named_child(module, parameter_index, "name")
                            .and_then(|name| self_ast_text(module, name))
                            .expect("validated self function parameter");
                        names.push(name.to_string());
                        defaulted.push(
                            !self_ast_children(module, parameter_index, "default").is_empty(),
                        );
                    }
                    (type_parameters, bounds, names, defaulted)
                })
            });
            body.push_str("{\"name\":");
            push_json_string(&mut body, &export.name);
            body.push_str(",\"type\":");
            push_json_string(&mut body, &semantic_type);
            let vars = function_shape
                .as_ref()
                .map(|(vars, _, _, _)| *vars)
                .unwrap_or(0);
            let required = function_shape
                .as_ref()
                .map(|(_, _, _, defaulted)| defaulted.iter().filter(|value| !**value).count())
                .unwrap_or(0);
            let _ = write!(body, ",\"vars\":{vars},\"bounds\":[");
            if let Some((_, bounds, _, _)) = &function_shape {
                for (parameter_index, parameter_bounds) in bounds.iter().enumerate() {
                    if parameter_index > 0 {
                        body.push(',');
                    }
                    body.push('[');
                    for (bound_index, bound) in parameter_bounds.iter().enumerate() {
                        if bound_index > 0 {
                            body.push(',');
                        }
                        push_json_string(&mut body, bound);
                    }
                    body.push(']');
                }
            }
            let _ = write!(body, "],\"required\":{required},\"namesKnown\":");
            body.push_str(if function_shape.is_some() {
                "true"
            } else {
                "false"
            });
            body.push_str(",\"names\":[");
            if let Some((_, _, names, _)) = &function_shape {
                for (index, name) in names.iter().enumerate() {
                    if index > 0 {
                        body.push(',');
                    }
                    push_json_string(&mut body, name);
                }
            }
            body.push_str("],\"defaulted\":[");
            if let Some((_, _, _, defaulted)) = &function_shape {
                for (index, defaulted) in defaulted.iter().enumerate() {
                    if index > 0 {
                        body.push(',');
                    }
                    body.push_str(if *defaulted { "true" } else { "false" });
                }
            }
            body.push_str("]}");
        }
        body.push_str("],\"aliases\":[");
        let aliases = exports
            .iter()
            .filter(|export| export.namespace == "type")
            .filter_map(|export| {
                let module = &product.typed().resolved.modules[export.module_index];
                let declaration_index = self_ast_declaration_index(
                    module,
                    export.declaration_lo,
                    export.declaration_hi,
                )?;
                (module.ast[declaration_index].kind == "statement/type-alias")
                    .then_some((*export, declaration_index))
            })
            .collect::<Vec<_>>();
        for (index, (export, declaration_index)) in aliases.iter().enumerate() {
            if index > 0 {
                body.push(',');
            }
            let module = &product.typed().resolved.modules[export.module_index];
            let params = self_ast_children(module, *declaration_index, "typeParameters").len();
            let ty = self_ast_named_child(module, *declaration_index, "type")
                .and_then(|node| self_ast_text(module, node))
                .expect("validated self type alias");
            body.push_str("{\"name\":");
            push_json_string(&mut body, &export.name);
            let _ = write!(body, ",\"params\":{params},\"type\":");
            push_json_string(&mut body, ty);
            body.push('}');
        }
        body.push_str("],\"records\":[");
        let records = exports
            .iter()
            .filter(|export| export.namespace == "type")
            .filter_map(|export| {
                let module = &product.typed().resolved.modules[export.module_index];
                let declaration_index = self_ast_declaration_index(
                    module,
                    export.declaration_lo,
                    export.declaration_hi,
                )?;
                (module.ast[declaration_index].kind == "statement/record")
                    .then_some((*export, declaration_index))
            })
            .collect::<Vec<_>>();
        for (index, (export, declaration_index)) in records.iter().enumerate() {
            if index > 0 {
                body.push(',');
            }
            let module = &product.typed().resolved.modules[export.module_index];
            let params = self_ast_children(module, *declaration_index, "typeParameters").len();
            body.push_str("{\"name\":");
            push_json_string(&mut body, &export.name);
            let _ = write!(body, ",\"params\":{params},\"fields\":[");
            for (field_index, (field_node_index, _)) in
                self_ast_children(module, *declaration_index, "fields")
                    .iter()
                    .enumerate()
            {
                if field_index > 0 {
                    body.push(',');
                }
                let name = self_ast_named_child(module, *field_node_index, "name")
                    .and_then(|node| self_ast_text(module, node))
                    .expect("validated self record field name");
                let ty = self_ast_named_child(module, *field_node_index, "type")
                    .and_then(|node| self_ast_text(module, node))
                    .expect("validated self record field type");
                body.push_str("{\"name\":");
                push_json_string(&mut body, name);
                body.push_str(",\"type\":");
                push_json_string(&mut body, ty);
                body.push_str(",\"hasDefault\":");
                body.push_str(
                    if self_ast_children(module, *field_node_index, "default").is_empty() {
                        "false"
                    } else {
                        "true"
                    },
                );
                body.push('}');
            }
            body.push_str("]}");
        }
        body.push_str("],\"enums\":[");
        let enums = exports
            .iter()
            .filter(|export| export.namespace == "type")
            .filter_map(|export| {
                let module = &product.typed().resolved.modules[export.module_index];
                let declaration_index = self_ast_declaration_index(
                    module,
                    export.declaration_lo,
                    export.declaration_hi,
                )?;
                (module.ast[declaration_index].kind == "statement/enum")
                    .then_some((*export, declaration_index))
            })
            .collect::<Vec<_>>();
        for (index, (export, declaration_index)) in enums.iter().enumerate() {
            if index > 0 {
                body.push(',');
            }
            let module = &product.typed().resolved.modules[export.module_index];
            let params = self_ast_children(module, *declaration_index, "typeParameters").len();
            body.push_str("{\"name\":");
            push_json_string(&mut body, &export.name);
            let _ = write!(body, ",\"params\":{params},\"variants\":[");
            for (variant_index, (variant_node_index, _)) in
                self_ast_children(module, *declaration_index, "variants")
                    .iter()
                    .enumerate()
            {
                if variant_index > 0 {
                    body.push(',');
                }
                let name = self_ast_named_child(module, *variant_node_index, "name")
                    .and_then(|node| self_ast_text(module, node))
                    .expect("validated self enum variant");
                body.push_str("{\"name\":");
                push_json_string(&mut body, name);
                body.push_str(",\"payloads\":[");
                for (payload_index, (_, payload)) in
                    self_ast_children(module, *variant_node_index, "payload")
                        .iter()
                        .enumerate()
                {
                    if payload_index > 0 {
                        body.push(',');
                    }
                    push_json_string(
                        &mut body,
                        self_ast_text(module, payload).expect("validated self enum payload"),
                    );
                }
                body.push_str("]}");
            }
            body.push_str("]}");
        }
        body.push_str("],\"newtypes\":[");
        let newtypes = exports
            .iter()
            .filter(|export| export.namespace == "type")
            .filter_map(|export| {
                let module = &product.typed().resolved.modules[export.module_index];
                let declaration_index = self_ast_declaration_index(
                    module,
                    export.declaration_lo,
                    export.declaration_hi,
                )?;
                (module.ast[declaration_index].kind == "statement/newtype")
                    .then_some((*export, declaration_index))
            })
            .collect::<Vec<_>>();
        for (index, (export, declaration_index)) in newtypes.iter().enumerate() {
            if index > 0 {
                body.push(',');
            }
            let module = &product.typed().resolved.modules[export.module_index];
            let params = self_ast_children(module, *declaration_index, "typeParameters").len();
            let base = self_ast_named_child(module, *declaration_index, "base")
                .and_then(|node| self_ast_text(module, node))
                .expect("validated self newtype");
            body.push_str("{\"name\":");
            push_json_string(&mut body, &export.name);
            let _ = write!(body, ",\"params\":{params},\"base\":");
            push_json_string(&mut body, base);
            body.push('}');
        }
        body.push_str("],\"conformances\":[]}");
        let digest = topaz_value::value::sha256(body.as_bytes());
        let mut signature = String::from("sha256:");
        topaz_value::bytes_to_hex_into(&mut signature, &digest);
        out.push_str("{\"identity\":");
        push_json_string(&mut out, identity);
        out.push_str(",\"signatureHash\":");
        push_json_string(&mut out, &signature);
        out.push_str(&body[body.find(",\"ambient\"").expect("self export body")..]);
    }
    out.push_str("]}");
    out
}

pub(super) fn render_semantic_type(ty: &topaz_hir::SemanticType) -> String {
    use topaz_hir::{SemanticConstructor as C, SemanticLiteral as L, SemanticPrimitive as P};

    let generic = |name: &str, arguments: &[topaz_hir::SemanticType]| {
        if arguments.is_empty() {
            name.to_string()
        } else {
            format!(
                "{name}<{}>",
                arguments
                    .iter()
                    .map(render_semantic_type)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    };
    match ty {
        topaz_hir::SemanticType::Primitive(value) => match value {
            P::Int => "int",
            P::Float => "float",
            P::String => "string",
            P::Bool => "bool",
            P::Unit => "unit",
        }
        .to_string(),
        topaz_hir::SemanticType::Literal(value) => match value {
            L::String(value) => format!("\"{value}\""),
            L::Int(value) => value.to_string(),
            L::Float(value) => value.clone(),
            L::Bool(value) => value.to_string(),
            L::Null => "null".to_string(),
        },
        topaz_hir::SemanticType::Union(values) => values
            .iter()
            .map(render_semantic_type)
            .collect::<Vec<_>>()
            .join(" | "),
        topaz_hir::SemanticType::Record(fields) => format!(
            "{{ {} }}",
            fields
                .iter()
                .map(|field| format!("{}: {}", field.name, render_semantic_type(&field.ty)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        topaz_hir::SemanticType::Constructor {
            constructor: C::Range,
            ..
        } => "range".to_string(),
        topaz_hir::SemanticType::Constructor {
            constructor,
            arguments,
        } => generic(
            match constructor {
                C::Array => "Array",
                C::Map => "Map",
                C::Set => "Set",
                C::Option => "Option",
                C::Result => "Result",
                C::Range => unreachable!(),
            },
            arguments,
        ),
        topaz_hir::SemanticType::Function {
            parameters,
            variadic,
            result,
        } => {
            let mut parameters = parameters
                .iter()
                .map(render_semantic_type)
                .collect::<Vec<_>>();
            if let Some(variadic) = variadic {
                parameters.push(format!("...{}", render_semantic_type(variadic)));
            }
            format!(
                "({}) -> {}",
                parameters.join(", "),
                render_semantic_type(result)
            )
        }
        topaz_hir::SemanticType::Foreign {
            identity,
            arguments,
        }
        | topaz_hir::SemanticType::Enum {
            identity,
            arguments,
        }
        | topaz_hir::SemanticType::NominalRecord {
            identity,
            arguments,
        }
        | topaz_hir::SemanticType::Newtype {
            identity,
            arguments,
        } => generic(identity, arguments),
        topaz_hir::SemanticType::Rigid { name, .. } => name.clone(),
        topaz_hir::SemanticType::Template => "template".to_string(),
        topaz_hir::SemanticType::File => "File".to_string(),
        topaz_hir::SemanticType::JsonValue => "JSONValue".to_string(),
        topaz_hir::SemanticType::Bytes => "Bytes".to_string(),
        topaz_hir::SemanticType::ByteBuffer => "ByteBuffer".to_string(),
        topaz_hir::SemanticType::Path => "Path".to_string(),
        topaz_hir::SemanticType::Regex => "Regex".to_string(),
        topaz_hir::SemanticType::Match => "Match".to_string(),
        topaz_hir::SemanticType::TomlValue => "TOMLValue".to_string(),
        topaz_hir::SemanticType::Url => "URL".to_string(),
        topaz_hir::SemanticType::Date => "Date".to_string(),
        topaz_hir::SemanticType::BigInt => "BigInt".to_string(),
        topaz_hir::SemanticType::Decimal => "Decimal".to_string(),
        topaz_hir::SemanticType::RoundingMode => "RoundingMode".to_string(),
        topaz_hir::SemanticType::Unknown => "?".to_string(),
        topaz_hir::SemanticType::InferenceVariable => "?inference".to_string(),
    }
}

pub(super) fn self_compilation_profile(
    profile: Option<profile::CheckProfile>,
) -> topaz_self_frontend::CompilationProfile {
    match profile {
        None => topaz_self_frontend::CompilationProfile::None,
        Some(profile::CheckProfile::AgentPack) => {
            topaz_self_frontend::CompilationProfile::AgentPack
        }
        Some(profile::CheckProfile::TestProfile) => {
            topaz_self_frontend::CompilationProfile::TestProfile
        }
        Some(profile::CheckProfile::Bootstrap) => {
            topaz_self_frontend::CompilationProfile::Bootstrap
        }
    }
}

pub(super) fn compile_self_product(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
    profile: Option<profile::CheckProfile>,
    recovery: &str,
) -> Result<topaz_self_frontend::SelfCompilationProduct, ExitCode> {
    let request = request.with_terminal_phase(topaz_kernel::TerminalPhase::RustSource);
    let product = match topaz_self_frontend::preview_linked_stage2_compilation_product(
        source,
        request,
        self_compilation_profile(profile),
    ) {
        Ok(product) => product,
        Err(error) => {
            eprintln!("topaz: self compiler stopped: {error}");
            eprintln!("topaz: recovery: {recovery} (not executed)");
            return Err(ExitCode::FAILURE);
        }
    };
    if let Err(error) = topaz_self_frontend::encode_self_compilation_product_manifest(&product) {
        eprintln!("topaz: self compilation product is invalid: {error}");
        eprintln!("topaz: recovery: {recovery} (not executed)");
        return Err(ExitCode::FAILURE);
    }
    trace_self_frontend_route("compilation-product");
    Ok(product)
}

pub(super) fn trace_self_frontend_route(route: &str) {
    if std::env::var_os("TOPAZ_SELF_FRONTEND_METRICS").is_some() {
        eprintln!("topaz-self-frontend-route: {route}");
    }
}

pub(super) fn compile_self_entry_product(
    entry: &str,
    root: Option<&str>,
    version: LangVersion,
    profile: Option<profile::CheckProfile>,
    command: &str,
) -> Result<topaz_self_frontend::SelfCompilationProduct, ExitCode> {
    let normalized = entry.replace('\\', "/");
    let (base, entry_relative, root_relative) = match split_absolute(&normalized, root) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("topaz: {error}");
            return Err(ExitCode::FAILURE);
        }
    };
    let request = topaz_kernel::KernelRequest::checked(
        &entry_relative,
        root_relative.as_deref(),
        version,
        topaz_kernel::PackageFacts::standalone(),
    );
    compile_self_product(
        &PhysicalFactHost::new(base),
        request,
        profile,
        &format!("rerun `topaz {command} {entry} --compiler rust`"),
    )
}

pub(super) fn compile_self_package_product(
    target: &PackageTarget,
    profile: Option<profile::CheckProfile>,
    command: &str,
) -> Result<topaz_self_frontend::SelfCompilationProduct, ExitCode> {
    let request = topaz_kernel::KernelRequest::checked(
        &target.entry,
        Some(""),
        target.version,
        package_kernel_facts(target),
    );
    compile_self_product(
        &PackageFactHost::new(target),
        request,
        profile,
        &format!("rerun `topaz {command} --compiler rust`"),
    )
}

pub(super) fn execute_self_product(
    product: topaz_self_frontend::SelfCompilationProduct,
    label: &str,
    program_args: &[String],
    test_mode: bool,
    presentation: CheckPresentation,
    host: std::rc::Rc<dyn Host>,
) -> ExitCode {
    if product.status() != "completed" {
        return check_self_compilation_product(product, label, false, false, presentation);
    }
    let runtime_inputs = match topaz_self_frontend::project_self_target_runtime_inputs(&product) {
        Ok(runtime_inputs) => runtime_inputs,
        Err(error) => {
            eprintln!("topaz: self product runtime stopped: {error}");
            eprintln!(
                "topaz: recovery: rerun the same command with `--compiler rust` (not executed)"
            );
            return ExitCode::FAILURE;
        }
    };
    if let Err(code) =
        admit_cli_program_args(runtime_inputs.facts.has_explicit_main(), program_args)
    {
        return code;
    }
    let stdin = host.input();
    let (value, explicit_main) =
        match topaz_self_frontend::execute_self_target_runtime_inputs_with_host_and_input(
            runtime_inputs,
            program_args,
            &stdin,
            host,
        ) {
            Ok(result) => result,
            Err(error) => {
                if let Some(runtime_error) =
                    topaz_self_frontend::decode_self_product_runtime_diagnostic(&error)
                {
                    let mut map = SourceMap::new();
                    for module in &product.typed().resolved.modules {
                        if map
                            .add_file(module.path.clone(), module.source.clone())
                            .is_err()
                        {
                            eprintln!("topaz: self product source map is invalid");
                            return ExitCode::FAILURE;
                        }
                    }
                    let diagnostic = Diagnostic::error(
                        Code::new(runtime_error.code),
                        runtime_error.message,
                        Label::new(runtime_error.span, ""),
                    );
                    eprintln!("{}", render(&diagnostic, &map));
                    return ExitCode::FAILURE;
                }
                eprintln!("topaz: self product runtime stopped: {error}");
                eprintln!(
                    "topaz: recovery: rerun the same command with `--compiler rust` (not executed)"
                );
                return ExitCode::FAILURE;
            }
        };
    let exit = explicit_main_exit(value, explicit_main);
    if test_mode && exit == ExitCode::SUCCESS {
        println!("{label}: test-ok");
    }
    exit
}

pub(super) fn run_self_entry(
    entry: &str,
    root: Option<&str>,
    version: LangVersion,
    program_args: &[String],
    test_mode: bool,
) -> ExitCode {
    let command = if test_mode { "test" } else { "run" };
    let product = match compile_self_entry_product(entry, root, version, None, command) {
        Ok(product) => product,
        Err(code) => return code,
    };
    let label = entry.replace('\\', "/");
    execute_self_product(
        product,
        &label,
        program_args,
        test_mode,
        CheckPresentation::Standalone,
        std::rc::Rc::new(NativeHost::new()),
    )
}

pub(super) fn run_self_package(
    target: &PackageTarget,
    program_args: &[String],
    test_mode: bool,
) -> ExitCode {
    let command = if test_mode { "test" } else { "run" };
    let product = match compile_self_package_product(target, None, command) {
        Ok(product) => product,
        Err(code) => return code,
    };
    let plan = if target.generated_std_modules.is_empty() {
        None
    } else {
        match checked_lispex_application_plan_from_targets(
            target,
            product
                .typed()
                .calls
                .iter()
                .filter_map(|call| call.target_identity.as_deref()),
        ) {
            Ok(plan) => Some(plan),
            Err(code) => return code,
        }
    };
    let native_host = NativeHost::with_fs_capabilities(
        &target.root,
        &target.fs_read_roots,
        &target.fs_write_roots,
    )
    .with_extern_replay(target.extern_replay.clone());
    let host: std::rc::Rc<dyn Host> = match plan.filter(|plan| !plan.rules.is_empty()) {
        Some(plan) => {
            let admitted = match admitted_lispex_application_rules(&plan) {
                Ok(admitted) => admitted,
                Err(code) => return code,
            };
            match topaz_lispex_embed::LispexApplicationHost::new(native_host, admitted, plan.quotas)
            {
                Ok(host) => std::rc::Rc::new(host),
                Err(error) => {
                    eprintln!("topaz: cannot create the checked Lispex application host: {error}");
                    return ExitCode::FAILURE;
                }
            }
        }
        None => std::rc::Rc::new(native_host),
    };
    execute_self_product(
        product,
        &target.entry,
        program_args,
        test_mode,
        CheckPresentation::Package,
        host,
    )
}

pub(super) fn check_unit(
    entry: &str,
    root: Option<&str>,
    version: LangVersion,
    types: bool,
    json: bool,
    exports_json: bool,
    compiler_selection: CompilerSelection,
) -> ExitCode {
    if version == LangVersion::V5_1 {
        eprintln!(
            "topaz: `check` needs v5.2+ (v5.16 is the default); `--language-version 5.1` has no module system (use `parse`)"
        );
        return ExitCode::FAILURE;
    }
    // Resolve absolute and relative entries the SAME way `run`/`emit`/`build` do
    // (split_absolute roots the provider at the entry's base), so `check <entry>`
    // and `run`/`emit <entry>` report the identical diagnostic stream for any path —
    // CDR-003 §13.6. (For a relative entry this is exactly the old `.`-rooted resolve.)
    let entry = entry.replace('\\', "/");
    let (base, entry_rel, root_rel) = match split_absolute(&entry, root) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("topaz: {msg}");
            return ExitCode::FAILURE;
        }
    };
    if version.uses_self_hosted_product_default() {
        let _ = types;
        let request = topaz_kernel::KernelRequest::checked(
            &entry_rel,
            root_rel.as_deref(),
            version,
            topaz_kernel::PackageFacts::standalone(),
        );
        return match compiler_selection {
            CompilerSelection::Rust => check_kernel_execution(
                topaz_kernel::drive_checked(&PhysicalFactHost::new(base), request),
                &entry,
                json,
                exports_json,
                CheckPresentation::Standalone,
            ),
            CompilerSelection::SelfHosted => {
                let product = match compile_self_product(
                    &PhysicalFactHost::new(base),
                    request,
                    None,
                    &format!("rerun `topaz check {entry} --compiler rust`"),
                ) {
                    Ok(product) => product,
                    Err(code) => return code,
                };
                check_self_compilation_product(
                    product,
                    &entry,
                    json,
                    exports_json,
                    CheckPresentation::Standalone,
                )
            }
        };
    }
    let provider = PhysicalProvider::new(&base);
    let out = resolve_with_version(&provider, &entry_rel, root_rel.as_deref(), version);
    for diag in &out.diagnostics {
        eprintln!(
            "{}",
            if json {
                render_json(diag, &out.map)
            } else {
                render(diag, &out.map)
            }
        );
    }
    if !has_errors(&out.diagnostics) {
        // CDR-004 C-6: `check` types the whole unit by default
        // (module-aware; `--types` is accepted as a no-op).
        let _ = types;
        let checked = match check_resolved_unit(&out, json, version) {
            Ok(checked) => checked,
            Err(n) => {
                // In JSON mode stderr is a pure JSONL diagnostic stream — the human
                // count line would break it; the exit code already signals failure.
                if !json {
                    eprintln!(
                        "{entry}: {n} type diagnostic{}",
                        if n == 1 { "" } else { "s" }
                    );
                }
                return ExitCode::FAILURE;
            }
        };
        if exports_json {
            println!("{}", render_export_surface_json(&checked.exports));
        } else if !json {
            println!(
                "{entry}: types-ok ({} module{})",
                out.modules.len(),
                if out.modules.len() == 1 { "" } else { "s" }
            );
            println!(
                "{entry}: resolve-ok ({} module{})",
                out.modules.len(),
                if out.modules.len() == 1 { "" } else { "s" }
            );
        }
        ExitCode::SUCCESS
    } else {
        if !json {
            eprintln!(
                "{entry}: {} diagnostic{}",
                out.diagnostics.len(),
                if out.diagnostics.len() == 1 { "" } else { "s" }
            );
        }
        ExitCode::FAILURE
    }
}

pub(super) fn check_unit_with_profile(
    entry: &str,
    root: Option<&str>,
    version: LangVersion,
    profile: profile::CheckProfile,
    json: bool,
    compiler_selection: CompilerSelection,
) -> ExitCode {
    debug_assert_eq!(version, LangVersion::CURRENT);
    let entry = entry.replace('\\', "/");
    let (base, entry_rel, root_rel) = match split_absolute(&entry, root) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("topaz: {message}");
            return ExitCode::FAILURE;
        }
    };
    let provider = PhysicalProvider::new(base.clone());
    if compiler_selection == CompilerSelection::SelfHosted {
        if profile == profile::CheckProfile::Bootstrap {
            eprintln!(
                "topaz: Bootstrap Profile applies only to a locked package; recovery: rerun a package check with `--compiler rust` (not executed)"
            );
            return ExitCode::FAILURE;
        }
        let request = topaz_kernel::KernelRequest::checked(
            &entry_rel,
            root_rel.as_deref(),
            version,
            topaz_kernel::PackageFacts::standalone(),
        );
        let product = match compile_self_product(
            &PhysicalFactHost::new(base),
            request,
            Some(profile),
            &format!("rerun `topaz check {entry} --compiler rust`"),
        ) {
            Ok(product) => product,
            Err(code) => return code,
        };
        return check_self_compilation_product(
            product,
            &entry,
            json,
            false,
            CheckPresentation::Standalone,
        );
    }
    let out = resolve_with_version(&provider, &entry_rel, root_rel.as_deref(), version);
    let mut findings = Vec::new();
    if profile == profile::CheckProfile::Bootstrap {
        let span = out
            .modules
            .iter()
            .find(|module| module.is_entry)
            .map(|module| module.program.span)
            .unwrap_or_else(|| topaz_diag::Span::new(topaz_diag::FileId(0), 0, 0));
        findings.push(profile::ProfileDiagnostic::policy(
            "bootstrap/requires-locked-package",
            "the Bootstrap Profile applies only to a locked package",
            span,
        ));
    }
    check_resolved_unit_with_profile(&out, &entry, version, profile, json, findings)
}

pub(super) fn check_resolved_unit_with_profile(
    out: &topaz_resolve::ResolveOutput,
    label: &str,
    version: LangVersion,
    selected: profile::CheckProfile,
    json: bool,
    extra_findings: Vec<profile::ProfileDiagnostic>,
) -> ExitCode {
    let mut diagnostics: Vec<profile::ProfileDiagnostic> = out
        .diagnostics
        .iter()
        .cloned()
        .map(profile::ProfileDiagnostic::compiler)
        .collect();

    let mut typed_hir = None;
    if !has_errors(&out.diagnostics) {
        let unit = unit_modules(out);
        let checked = topaz_check::check_unit_typed_with_version(&unit, version);
        diagnostics.extend(
            checked
                .diagnostics
                .iter()
                .cloned()
                .map(profile::ProfileDiagnostic::compiler),
        );
        typed_hir = checked.typed_hir;
    }
    diagnostics.extend(extra_findings);
    diagnostics.extend(profile::collect(out, selected));
    diagnostics.extend(profile::collect_typed(selected, typed_hir.as_ref()));
    let mut seen = BTreeSet::new();
    diagnostics.retain(|finding| {
        let span = finding.diagnostic.primary.span;
        seen.insert((
            span.file.0,
            span.lo,
            span.hi,
            finding.rule,
            finding.diagnostic.code.as_str(),
            finding.diagnostic.message.clone(),
        ))
    });

    for finding in &diagnostics {
        eprintln!(
            "{}",
            profile::render_profile_diagnostic(selected, finding, &out.map, json)
        );
    }

    let failed = diagnostics
        .iter()
        .any(|finding| has_errors(std::slice::from_ref(&finding.diagnostic)));
    if json {
        println!(
            "{}",
            profile::render_summary(selected, version, &diagnostics)
        );
    } else if failed {
        eprintln!(
            "profile[{}] {label}: {} diagnostic{}",
            selected.as_str(),
            diagnostics.len(),
            if diagnostics.len() == 1 { "" } else { "s" }
        );
    } else {
        println!(
            "profile[{}] {label}: types-ok ({} module{})",
            selected.as_str(),
            out.modules.len(),
            if out.modules.len() == 1 { "" } else { "s" }
        );
        println!(
            "profile[{}] {label}: resolve-ok ({} module{})",
            selected.as_str(),
            out.modules.len(),
            if out.modules.len() == 1 { "" } else { "s" }
        );
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
