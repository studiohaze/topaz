use crate::*;

pub(super) fn run_package_target(
    target: &PackageTarget,
    unchecked: bool,
    program_args: &[String],
) -> ExitCode {
    let host = NativeHost::with_fs_capabilities(
        &target.root,
        &target.fs_read_roots,
        &target.fs_write_roots,
    )
    .with_extern_replay(target.extern_replay.clone());
    let unit = resolve_package_target(target);
    for diag in &unit.diagnostics {
        eprintln!("{}", render(diag, &unit.map));
    }
    if has_errors(&unit.diagnostics) {
        return ExitCode::FAILURE;
    }
    if unchecked && !target.generated_std_modules.is_empty() {
        eprintln!(
            "topaz: --unchecked cannot run a package with the first-class Lispex application capability"
        );
        return ExitCode::FAILURE;
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
                return ExitCode::FAILURE;
            }
        }
    };
    let invocation = ResolvedUnitInvocation::new(&unit);
    if let Err(code) = admit_cli_program_args(invocation.has_explicit_main(), program_args) {
        return code;
    }
    let plan = match checked
        .as_ref()
        .map(|checked| checked_lispex_application_plan(target, checked))
        .transpose()
    {
        Ok(plan) => plan.flatten(),
        Err(code) => return code,
    };
    let outcome = if let Some(plan) = plan.filter(|plan| !plan.rules.is_empty()) {
        let admitted = match admitted_lispex_application_rules(&plan) {
            Ok(admitted) => admitted,
            Err(code) => return code,
        };
        let host = match topaz_lispex_embed::LispexApplicationHost::new(host, admitted, plan.quotas)
        {
            Ok(host) => host,
            Err(error) => {
                eprintln!("topaz: cannot create the checked Lispex application host: {error}");
                return ExitCode::FAILURE;
            }
        };
        invocation.run(&host, program_args)
    } else {
        invocation.run(&host, program_args)
    };
    match outcome {
        Ok(value) => explicit_main_exit(value, invocation.has_explicit_main()),
        Err(e) => {
            let diag =
                Diagnostic::error(Code::new(e.code), e.message.clone(), Label::new(e.span, ""));
            eprintln!("{}", render(&diag, &unit.map));
            ExitCode::FAILURE
        }
    }
}

pub(super) fn checked_lispex_application_plan(
    target: &PackageTarget,
    checked: &topaz_check::CheckedUnit,
) -> Result<Option<topaz_lispex_product::CheckedApplicationPlan>, ExitCode> {
    if target.generated_std_modules.is_empty() {
        return Ok(None);
    }
    let typed = checked.typed_hir.as_ref().ok_or_else(|| {
        eprintln!("topaz: a clean Lispex application check produced no typed call facts");
        ExitCode::FAILURE
    })?;
    checked_lispex_application_plan_from_targets(
        target,
        typed
            .calls
            .iter()
            .filter_map(|call| call.target_identity.as_deref()),
    )
    .map(Some)
}

pub(super) fn checked_lispex_application_plan_from_targets<'a>(
    target: &PackageTarget,
    call_target_identities: impl IntoIterator<Item = &'a str>,
) -> Result<topaz_lispex_product::CheckedApplicationPlan, ExitCode> {
    let project = topaz_package::Project::load(&target.root).map_err(|error| {
        eprintln!("topaz: cannot reopen the checked package for Lispex planning: {error}");
        ExitCode::FAILURE
    })?;
    topaz_lispex_product::checked_application_plan(&project, call_target_identities).map_err(
        |error| {
            eprintln!("topaz: cannot derive the checked Lispex application plan: {error}");
            ExitCode::FAILURE
        },
    )
}

pub(super) fn self_lispex_application_plan(
    target: &PackageTarget,
    product: &topaz_self_frontend::SelfCompilationProduct,
) -> Result<Option<topaz_lispex_product::CheckedApplicationPlan>, ExitCode> {
    if target.generated_std_modules.is_empty() {
        return Ok(None);
    }
    checked_lispex_application_plan_from_targets(
        target,
        product
            .typed()
            .calls
            .iter()
            .filter_map(|call| call.target_identity.as_deref()),
    )
    .map(Some)
}

pub(super) fn reject_self_lispex_application_target(
    plan: Option<&topaz_lispex_product::CheckedApplicationPlan>,
    build_target: &str,
) -> Result<(), ExitCode> {
    if plan.is_some_and(|plan| !plan.reachable_rules.is_empty()) {
        eprintln!(
            "topaz: `{build_target}` does not admit the first-class Lispex application capability"
        );
        return Err(ExitCode::FAILURE);
    }
    Ok(())
}

pub(super) fn admitted_lispex_application_rules(
    plan: &topaz_lispex_product::CheckedApplicationPlan,
) -> Result<Vec<topaz_lispex_embed::AdmittedApplicationRule>, ExitCode> {
    plan.rules
        .iter()
        .map(|rule| {
            topaz_lispex_embed::AdmittedApplicationRule::from_locked_artifact(
                rule.identity.clone(),
                &rule.preparation_submission_sha256,
                &rule.prepared_artifact,
                rule.limits,
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            eprintln!("topaz: cannot admit the checked Lispex rule plan: {error}");
            ExitCode::FAILURE
        })
}

pub(super) fn reject_reached_lispex_application_target(
    target: &PackageTarget,
    checked: &topaz_check::CheckedUnit,
    build_target: &str,
) -> Result<(), ExitCode> {
    if checked_lispex_application_plan(target, checked)?
        .as_ref()
        .is_some_and(|plan| !plan.reachable_rules.is_empty())
    {
        eprintln!(
            "topaz: `{build_target}` does not admit the first-class Lispex application capability"
        );
        return Err(ExitCode::FAILURE);
    }
    Ok(())
}

/// Execute one resolver-owned unit through the CLI entry boundary. The
/// resolver decides whether the entry exports `main`; every interpreter-backed
/// CLI consumer then supplies arguments and host input through this one path.
pub(super) struct ResolvedUnitInvocation<'a> {
    pub(super) unit: &'a topaz_resolve::ResolveOutput,
    pub(super) explicit_main: bool,
}

impl<'a> ResolvedUnitInvocation<'a> {
    pub(super) fn new(unit: &'a topaz_resolve::ResolveOutput) -> Self {
        Self {
            unit,
            explicit_main: topaz_resolve::has_explicit_main(unit),
        }
    }

    pub(super) fn has_explicit_main(&self) -> bool {
        self.explicit_main
    }

    pub(super) fn run(
        &self,
        host: &dyn Host,
        program_args: &[String],
    ) -> Result<Value, topaz_interp::RtError> {
        if self.explicit_main {
            let stdin = host.input();
            Machine::run_unit_with_main(self.unit, host, program_args, &stdin)
        } else {
            Machine::run_unit(self.unit, host)
        }
    }
}

/// `topaz run <entry>` resolves and executes a v5.2+ module unit, while v5.1
/// executes the entry through its frozen single-file parser and interpreter.
pub(super) fn run_entry(
    entry: &str,
    root: Option<&str>,
    version: LangVersion,
    unchecked: bool,
    program_args: &[String],
) -> ExitCode {
    let host = NativeHost::new();
    if version != LangVersion::V5_1 {
        // v5.2+ all use the module system; v5.3 adds user enums; v5.4 adds
        // multi-payload/recursive enums; v5.5 promotes backend parity, v5.6
        // activates its frozen authority, v5.7 inherits that complete surface,
        // v5.8 adds the lifecycle-v2 local-data host contract, v5.9 adds the
        // bounded HTTP service contract, v5.17 adds the bounded Lispex
        // product, v5.18 activates its first-class bounded application boundary,
        // v5.19 integrates the complete-profile application, and current v5.20
        // advances nominal declaration identity. The selected
        // version is threaded to the resolver + checker so an explicit older
        // selector gates out later features.
        // Absolute entries/roots get a provider rooted at the
        // right base so provider paths stay relative.
        let entry_norm = entry.replace('\\', "/");
        let (base, entry_rel, root_rel) = match split_absolute(&entry_norm, root) {
            Ok(v) => v,
            Err(msg) => {
                eprintln!("topaz: {msg}");
                return ExitCode::FAILURE;
            }
        };
        let provider = PhysicalProvider::new(base);
        let unit = resolve_with_version(&provider, &entry_rel, root_rel.as_deref(), version);
        for diag in &unit.diagnostics {
            eprintln!("{}", render(diag, &unit.map));
        }
        if has_errors(&unit.diagnostics) {
            return ExitCode::FAILURE;
        }
        // CDR-003 §13: `run` statically type-checks the unit by default — the
        // same gate as `topaz check`, reported identically. `--unchecked` opts
        // out for the runtime-semantics workflow (the interpreter still runs
        // type-incorrect programs to a fault).
        if !unchecked && let Err(n) = check_resolved_unit(&unit, false, version) {
            eprintln!(
                "{entry_norm}: {n} type diagnostic{}",
                if n == 1 { "" } else { "s" }
            );
            return ExitCode::FAILURE;
        }
        let invocation = ResolvedUnitInvocation::new(&unit);
        if let Err(code) = admit_cli_program_args(invocation.has_explicit_main(), program_args) {
            return code;
        }
        let outcome = invocation.run(&host, program_args);
        match outcome {
            Ok(value) => explicit_main_exit(value, invocation.has_explicit_main()),
            Err(e) => {
                let diag =
                    Diagnostic::error(Code::new(e.code), e.message.clone(), Label::new(e.span, ""));
                eprintln!("{}", render(&diag, &unit.map));
                ExitCode::FAILURE
            }
        }
    } else {
        // v5.1 is the frozen single-file mode with no module system and no
        // checker, so `--unchecked` is a no-op here (nothing to skip).
        let Ok(src) = std::fs::read_to_string(entry) else {
            eprintln!("topaz: cannot read `{entry}`");
            return ExitCode::FAILURE;
        };
        let out = topaz_parser::parse(FileId(0), &src);
        if !out.diagnostics.is_empty() {
            let mut map = SourceMap::new();
            let _ = map.add_file(entry.to_string(), src.clone());
            for diag in &out.diagnostics {
                eprintln!("{}", render(diag, &map));
            }
            if has_errors(&out.diagnostics) {
                return ExitCode::FAILURE;
            }
        }
        let mut machine = Machine::new(&src, &host);
        match machine.run_program(&out.program) {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                let mut map = SourceMap::new();
                let _ = map.add_file(entry.to_string(), src.clone());
                let diag =
                    Diagnostic::error(Code::new(e.code), e.message.clone(), Label::new(e.span, ""));
                eprintln!("{}", render(&diag, &map));
                ExitCode::FAILURE
            }
        }
    }
}
