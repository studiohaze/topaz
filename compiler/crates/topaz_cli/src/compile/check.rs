use crate::*;

#[derive(Debug, Clone, Copy)]
pub(super) enum CheckPresentation {
    Standalone,
    Package,
}

pub(super) fn check_kernel_execution(
    execution: topaz_kernel::KernelExecution,
    label: &str,
    json: bool,
    exports_json: bool,
    presentation: CheckPresentation,
) -> ExitCode {
    let unit = match execution.outcome {
        topaz_kernel::KernelOutcome::Completed(unit)
        | topaz_kernel::KernelOutcome::Rejected(unit) => unit,
        topaz_kernel::KernelOutcome::NeedHostFacts(queries) => {
            eprintln!(
                "topaz: compiler kernel stopped with {} unsatisfied host fact{}",
                queries.len(),
                if queries.len() == 1 { "" } else { "s" }
            );
            return ExitCode::FAILURE;
        }
        topaz_kernel::KernelOutcome::Declined { reason } => {
            eprintln!("topaz: compiler kernel declined current check: {reason}");
            return ExitCode::FAILURE;
        }
        topaz_kernel::KernelOutcome::ResourceLimit(limit) => {
            eprintln!(
                "topaz: compiler kernel resource limit {:?}: observed {}, limit {}",
                limit.dimension, limit.observed, limit.limit
            );
            return ExitCode::FAILURE;
        }
        topaz_kernel::KernelOutcome::CompilerFault { message } => {
            eprintln!("topaz: compiler kernel fault: {message}");
            return ExitCode::FAILURE;
        }
    };
    let out = &unit.resolved;
    for diagnostic in &out.diagnostics {
        eprintln!(
            "{}",
            if json {
                render_json(diagnostic, &out.map)
            } else {
                render(diagnostic, &out.map)
            }
        );
    }
    if has_errors(&out.diagnostics) {
        if !json && matches!(presentation, CheckPresentation::Standalone) {
            eprintln!(
                "{label}: {} diagnostic{}",
                out.diagnostics.len(),
                if out.diagnostics.len() == 1 { "" } else { "s" }
            );
        }
        return ExitCode::FAILURE;
    }
    let Some(checked) = unit.checked.as_ref() else {
        eprintln!("topaz: compiler kernel completed a clean resolve without a check result");
        return ExitCode::FAILURE;
    };
    for diagnostic in &checked.diagnostics {
        eprintln!(
            "{}",
            if json {
                render_json(diagnostic, &out.map)
            } else {
                render(diagnostic, &out.map)
            }
        );
    }
    if has_errors(&checked.diagnostics) {
        if !json {
            eprintln!(
                "{label}: {} type diagnostic{}",
                checked.diagnostics.len(),
                if checked.diagnostics.len() == 1 {
                    ""
                } else {
                    "s"
                }
            );
        }
        return ExitCode::FAILURE;
    }
    if exports_json {
        println!("{}", render_export_surface_json(&checked.exports));
    } else if !json && matches!(presentation, CheckPresentation::Standalone) {
        println!(
            "{label}: types-ok ({} module{})",
            out.modules.len(),
            if out.modules.len() == 1 { "" } else { "s" }
        );
        println!(
            "{label}: resolve-ok ({} module{})",
            out.modules.len(),
            if out.modules.len() == 1 { "" } else { "s" }
        );
    }
    ExitCode::SUCCESS
}

pub(super) fn check_package_target(
    target: &PackageTarget,
    types: bool,
    json: bool,
    exports_json: bool,
    compiler_selection: CompilerSelection,
) -> ExitCode {
    if target.version.uses_self_hosted_product_default() {
        let _ = types;
        let host = PackageFactHost::new(target);
        let request = topaz_kernel::KernelRequest::checked(
            &target.entry,
            Some(""),
            target.version,
            package_kernel_facts(target),
        );
        return match compiler_selection {
            CompilerSelection::Rust => check_kernel_execution(
                topaz_kernel::drive_checked(&host, request),
                &target.entry,
                json,
                exports_json,
                CheckPresentation::Package,
            ),
            CompilerSelection::SelfHosted => {
                let product = match compile_self_product(
                    &host,
                    request,
                    None,
                    "rerun `topaz check --compiler rust`",
                ) {
                    Ok(product) => product,
                    Err(code) => return code,
                };
                check_self_compilation_product(
                    product,
                    &target.entry,
                    json,
                    exports_json,
                    CheckPresentation::Package,
                )
            }
        };
    }
    let out = resolve_package_target(target);
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
        let _ = types;
        match check_resolved_unit(&out, json, target.version) {
            Ok(checked) => {
                if exports_json {
                    println!("{}", render_export_surface_json(&checked.exports));
                }
            }
            Err(n) => {
                if !json {
                    eprintln!(
                        "{}: {n} type diagnostic{}",
                        target.entry,
                        if n == 1 { "" } else { "s" }
                    );
                }
                return ExitCode::FAILURE;
            }
        }
    }
    if has_errors(&out.diagnostics) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

pub(super) fn check_package_target_with_profile(
    target: &PackageTarget,
    profile: profile::CheckProfile,
    json: bool,
    compiler_selection: CompilerSelection,
) -> ExitCode {
    let required_version = if profile == profile::CheckProfile::Bootstrap {
        LangVersion::UNMARKED_SOURCE
    } else {
        LangVersion::CURRENT
    };
    if target.version != required_version {
        eprintln!(
            "topaz: `--profile` requires canonical language `topaz-{}` (package declares `topaz-{}`)",
            required_version.as_str(),
            lang_version_text(target.version)
        );
        return ExitCode::FAILURE;
    }
    if compiler_selection == CompilerSelection::SelfHosted {
        if profile == profile::CheckProfile::Bootstrap
            && (!target.locked
                || !target.build_deterministic
                || !target.externs.is_empty()
                || !target.fs_read_roots.is_empty()
                || !target.fs_write_roots.is_empty()
                || target.web_capabilities.open_text
                || target.web_capabilities.download_text
                || target.web_capabilities.local_state)
        {
            eprintln!(
                "topaz: Bootstrap Profile package policy failed before self compilation; recovery: rerun `topaz check --profile bootstrap --locked --compiler rust` (not executed)"
            );
            return ExitCode::FAILURE;
        }
        let host = PackageFactHost::new(target);
        let request = topaz_kernel::KernelRequest::checked(
            &target.entry,
            Some(""),
            target.version,
            package_kernel_facts(target),
        );
        let product = match compile_self_product(
            &host,
            request,
            Some(profile),
            "rerun `topaz check --compiler rust`",
        ) {
            Ok(product) => product,
            Err(code) => return code,
        };
        return check_self_compilation_product(
            product,
            &target.entry,
            json,
            false,
            CheckPresentation::Package,
        );
    }
    let out = resolve_package_target(target);
    let span = out
        .modules
        .iter()
        .find(|module| module.is_entry)
        .map(|module| module.program.span)
        .unwrap_or_else(|| topaz_diag::Span::new(topaz_diag::FileId(0), 0, 0));
    let mut package_findings = Vec::new();
    if profile == profile::CheckProfile::Bootstrap {
        if !target.locked {
            package_findings.push(profile::ProfileDiagnostic::policy(
                "bootstrap/requires-locked-package",
                "the Bootstrap Profile requires `topaz check --profile bootstrap --locked`",
                span,
            ));
        }
        if !target.build_deterministic {
            package_findings.push(profile::ProfileDiagnostic::policy(
                "bootstrap/requires-deterministic-build",
                "the Bootstrap Profile requires `[build].deterministic = true`",
                span,
            ));
        }
        if !target.externs.is_empty() {
            package_findings.push(profile::ProfileDiagnostic::policy(
                "bootstrap/no-extern",
                "manifest extern modules are not available to the Bootstrap Profile",
                span,
            ));
        }
        let has_capability = !target.fs_read_roots.is_empty()
            || !target.fs_write_roots.is_empty()
            || target.web_capabilities.open_text
            || target.web_capabilities.download_text
            || target.web_capabilities.local_state;
        if has_capability {
            package_findings.push(profile::ProfileDiagnostic::policy(
                "bootstrap/no-capability",
                "host capabilities are not available to the Bootstrap Profile",
                span,
            ));
        }
    }
    check_resolved_unit_with_profile(
        &out,
        &target.entry,
        target.version,
        profile,
        json,
        package_findings,
    )
}

/// Splits possibly-absolute entry/root into a provider base plus
/// provider-relative paths.
pub(super) fn split_absolute(
    entry: &str,
    root: Option<&str>,
) -> Result<(String, String, Option<String>), String> {
    let entry_path = std::path::Path::new(entry);
    if let Some(root) = root
        && !root.is_empty()
        && topaz_resolve::normalize_path(entry) == topaz_resolve::normalize_path(root)
    {
        return Err(format!(
            "the source root `{root}` must be a directory containing the entry, not the entry file itself"
        ));
    }
    if !entry_path.is_absolute() {
        // A relative entry outside an explicit `--root` must fail as a PLAIN CLI
        // error, exactly like an absolute one — not as a resolver diagnostic that
        // loads the entry source only to anchor a misleading caret on byte 0. Mirror
        // the resolver's lexical containment (SPEC v5.2 §17) here so the two agree.
        if let Some(root) = root {
            let entry_norm = topaz_resolve::normalize_path(entry);
            let root_norm = topaz_resolve::normalize_path(root);
            let contained =
                root_norm.is_empty() || entry_norm.starts_with(&format!("{root_norm}/"));
            if !contained {
                return Err(format!("entry `{entry}` is not under --root `{root}`"));
            }
        }
        return Ok((".".into(), entry.to_string(), root.map(String::from)));
    }
    match root {
        Some(root) => {
            let root_norm = root.replace('\\', "/");
            // Component-aware containment: `C:/root2/x` is NOT under
            // `C:/root`.
            let rel = std::path::Path::new(entry)
                .strip_prefix(std::path::Path::new(&root_norm))
                .map_err(|_| format!("entry `{entry}` is not under --root `{root_norm}`"))?;
            Ok((
                root_norm,
                rel.to_string_lossy().replace('\\', "/"),
                Some(String::new()),
            ))
        }
        None => {
            let parent = entry_path
                .parent()
                .ok_or_else(|| format!("entry `{entry}` has no parent directory"))?;
            let name = entry_path
                .file_name()
                .ok_or_else(|| format!("entry `{entry}` has no file name"))?;
            Ok((
                parent.to_string_lossy().replace('\\', "/"),
                name.to_string_lossy().into_owned(),
                None,
            ))
        }
    }
}

/// Project resolver-owned modules into the checker input used by every CLI
/// consumer, preserving the same identity, source, and module-role facts.
pub(super) fn unit_modules(out: &topaz_resolve::ResolveOutput) -> Vec<topaz_check::UnitModule<'_>> {
    out.modules
        .iter()
        .map(|module| topaz_check::UnitModule {
            identity: module.identity.clone(),
            is_entry: module.is_entry,
            is_extern: module.is_extern,
            is_generated_std: module.is_generated_std,
            extern_replay_error: module.extern_replay_error.clone(),
            src: out.map.file(module.file).src(),
            program: &module.program,
        })
        .collect()
}

/// Type-check an already-resolved unit with `topaz_check`, rendering any type
/// diagnostics to stderr exactly as `check` does. Returns the clean
/// [`topaz_check::CheckOutput`] so `check --exports-json` can print the public
/// surface; returns `Err(n)` (the diagnostic count) when the unit does not type.
/// Shared by `check` and the checked-by-default `run`/`emit`/`build` (CDR-003
/// §13): the checker is the single admission gate for the CLI, so all four
/// commands report type errors identically. The interpreter/emitter differential
/// harness keeps its own checker-free path (CDR-006 §7) and does not call this.
pub(super) fn check_resolved_unit(
    out: &topaz_resolve::ResolveOutput,
    json: bool,
    version: LangVersion,
) -> Result<topaz_check::CheckedUnit, usize> {
    let unit = unit_modules(out);
    let checked = topaz_check::check_unit_typed_with_version(&unit, version);
    for diag in &checked.diagnostics {
        eprintln!(
            "{}",
            if json {
                render_json(diag, &out.map)
            } else {
                render(diag, &out.map)
            }
        );
    }
    if has_errors(&checked.diagnostics) {
        Err(checked.diagnostics.len())
    } else {
        Ok(checked)
    }
}

pub(super) fn render_export_surface_json(
    exports: &BTreeMap<String, topaz_check::ModuleExports>,
) -> String {
    let mut out = String::from("{\"modules\":[");
    for (i, (identity, surface)) in exports.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        render_export_module_json(&mut out, identity, surface, true);
    }
    out.push_str("]}");
    out
}

pub(super) fn render_export_module_json(
    out: &mut String,
    identity: &str,
    surface: &topaz_check::ModuleExports,
    include_signature_hash: bool,
) {
    out.push_str("{\"identity\":");
    push_json_string(out, identity);
    if include_signature_hash {
        out.push_str(",\"signatureHash\":");
        let signature_hash = export_module_signature_hash(identity, surface);
        push_json_string(out, &signature_hash);
    }
    out.push_str(",\"ambient\":");
    out.push_str(if surface.ambient { "true" } else { "false" });
    out.push_str(",\"values\":[");
    let mut values: Vec<_> = surface.values.iter().collect();
    values.sort_by_key(|(name, _)| *name);
    for (j, (name, value)) in values.iter().enumerate() {
        if j > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_json_string(out, name);
        out.push_str(",\"type\":");
        push_json_string(out, &value.ty.to_string());
        let _ = write!(out, ",\"vars\":{},\"bounds\":[", value.vars);
        for (k, bounds) in value.bounds.iter().enumerate() {
            if k > 0 {
                out.push(',');
            }
            out.push('[');
            for (l, bound) in bounds.iter().enumerate() {
                if l > 0 {
                    out.push(',');
                }
                push_json_string(out, bound);
            }
            out.push(']');
        }
        let _ = write!(out, "],\"required\":{}", value.required);
        out.push_str(",\"namesKnown\":");
        out.push_str(if value.names_known { "true" } else { "false" });
        out.push_str(",\"names\":[");
        for (k, name) in value.names.iter().enumerate() {
            if k > 0 {
                out.push(',');
            }
            push_json_string(out, name);
        }
        out.push_str("],\"defaulted\":[");
        for (k, defaulted) in value.defaulted.iter().enumerate() {
            if k > 0 {
                out.push(',');
            }
            out.push_str(if *defaulted { "true" } else { "false" });
        }
        out.push_str("]}");
    }
    out.push_str("],\"aliases\":[");
    let mut aliases: Vec<_> = surface.aliases.iter().collect();
    aliases.sort_by_key(|(name, _)| *name);
    for (j, (name, alias)) in aliases.iter().enumerate() {
        if j > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_json_string(out, name);
        let _ = write!(out, ",\"params\":{},\"type\":", alias.params);
        push_json_string(out, &alias.body.to_string());
        out.push('}');
    }
    out.push_str("],\"records\":[");
    let mut records: Vec<_> = surface.records.iter().collect();
    records.sort_by_key(|(name, _)| *name);
    for (j, (name, record)) in records.iter().enumerate() {
        if j > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_json_string(out, name);
        let _ = write!(out, ",\"params\":{}", record.params);
        out.push_str(",\"fields\":[");
        for (k, field) in record.fields.iter().enumerate() {
            if k > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":");
            push_json_string(out, &field.name);
            out.push_str(",\"type\":");
            push_json_string(out, &field.ty.to_string());
            out.push_str(",\"hasDefault\":");
            out.push_str(if field.has_default { "true" } else { "false" });
            out.push('}');
        }
        out.push_str("]}");
    }
    out.push_str("],\"enums\":[");
    let mut enums: Vec<_> = surface.enums.iter().collect();
    enums.sort_by_key(|(name, _)| *name);
    for (j, (name, enm)) in enums.iter().enumerate() {
        if j > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_json_string(out, name);
        let _ = write!(out, ",\"params\":{}", enm.params);
        out.push_str(",\"variants\":[");
        for (k, variant) in enm.variants.iter().enumerate() {
            if k > 0 {
                out.push(',');
            }
            out.push_str("{\"name\":");
            push_json_string(out, &variant.name);
            out.push_str(",\"payloads\":[");
            for (l, payload) in variant.payloads.iter().enumerate() {
                if l > 0 {
                    out.push(',');
                }
                push_json_string(out, &payload.to_string());
            }
            out.push_str("]}");
        }
        out.push_str("]}");
    }
    out.push_str("],\"newtypes\":[");
    let mut newtypes: Vec<_> = surface.newtypes.iter().collect();
    newtypes.sort_by_key(|(name, _)| *name);
    for (j, (name, newtype)) in newtypes.iter().enumerate() {
        if j > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_json_string(out, name);
        let _ = write!(out, ",\"params\":{}", newtype.params);
        out.push_str(",\"base\":");
        push_json_string(out, &newtype.base.to_string());
        out.push('}');
    }
    out.push_str("],\"conformances\":[");
    let mut conformances = surface.conformances.clone();
    conformances.sort();
    conformances.dedup();
    for (j, (protocol, ty)) in conformances.iter().enumerate() {
        if j > 0 {
            out.push(',');
        }
        out.push_str("{\"protocol\":");
        push_json_string(out, protocol);
        out.push_str(",\"type\":");
        push_json_string(out, ty);
        out.push('}');
    }
    out.push_str("]}");
}

pub(super) fn export_module_signature_hash(
    identity: &str,
    surface: &topaz_check::ModuleExports,
) -> String {
    let mut canonical = String::new();
    render_export_module_json(&mut canonical, identity, surface, false);
    let digest = topaz_value::value::sha256(canonical.as_bytes());
    let mut out = String::from("sha256:");
    topaz_value::bytes_to_hex_into(&mut out, &digest);
    out
}

pub(super) fn push_json_string(out: &mut String, raw: &str) {
    out.push('"');
    for c in raw.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
