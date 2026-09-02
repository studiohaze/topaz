use crate::*;

pub(super) fn test_package_target(target: &PackageTarget, program_args: &[String]) -> ExitCode {
    let host = cli_test_host();
    host.set_extern_replay(target.extern_replay.clone());
    let unit = resolve_package_target(target);
    for diag in &unit.diagnostics {
        eprintln!("{}", render(diag, &unit.map));
    }
    if has_errors(&unit.diagnostics) {
        return ExitCode::FAILURE;
    }
    if let Err(n) = check_resolved_unit(&unit, false, target.version) {
        eprintln!(
            "{}: {n} type diagnostic{}",
            target.entry,
            if n == 1 { "" } else { "s" }
        );
        return ExitCode::FAILURE;
    }
    let invocation = ResolvedUnitInvocation::new(&unit);
    if let Err(code) = admit_cli_program_args(invocation.has_explicit_main(), program_args) {
        return code;
    }
    let outcome = invocation.run(&host, program_args);
    finish_test_run(
        &target.entry,
        &unit,
        &host,
        invocation.has_explicit_main(),
        outcome,
    )
}

/// Run the deterministic test host with the process-input contract exposed by
/// the native CLI host. This snapshots piped stdin once while preserving the
/// non-blocking empty input used for an interactive terminal.
pub(super) fn cli_test_host() -> topaz_interp::TestHost {
    let host = topaz_interp::TestHost::new();
    host.set_input(NativeHost::new().input());
    host
}

pub(super) fn test_selected_entry(
    entry: &str,
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    version: LangVersion,
    locked: bool,
    program_args: &[String],
    compiler_selection: CompilerSelection,
) -> ExitCode {
    if let Some(root) = root
        && Path::new(root).join("topaz.toml").is_file()
    {
        let mut target = match package_target(Some(root), version_arg, locked) {
            Ok(target) => target,
            Err(code) => return code,
        };
        let candidate = PathBuf::from(entry);
        let candidate = if candidate.is_absolute() {
            candidate
        } else {
            target.root.join(candidate)
        };
        let canonical = match fs::canonicalize(&candidate) {
            Ok(path) if path.is_file() => path,
            Ok(_) => {
                eprintln!(
                    "topaz: package test entry `{entry}` is not a file under `{}`",
                    target.root.to_string_lossy()
                );
                return ExitCode::FAILURE;
            }
            Err(error) => {
                eprintln!(
                    "topaz: cannot resolve package test entry `{entry}` under `{}`: {error}",
                    target.root.to_string_lossy()
                );
                return ExitCode::FAILURE;
            }
        };
        let requested_root = PathBuf::from(root);
        let logical_root = if requested_root.is_absolute() {
            requested_root
        } else {
            match std::env::current_dir() {
                Ok(current) => current.join(requested_root),
                Err(error) => {
                    eprintln!("topaz: cannot resolve current directory for package test: {error}");
                    return ExitCode::FAILURE;
                }
            }
        };
        let relative = match logical_root_relative_path(
            &logical_root,
            &target.root,
            &candidate,
            Some(&canonical),
        ) {
            Some(path) if path.extension().and_then(|ext| ext.to_str()) == Some("tpz") => path,
            _ => {
                eprintln!(
                    "topaz: package test entry `{entry}` must be a `.tpz` file inside `{}`",
                    target.root.to_string_lossy()
                );
                return ExitCode::FAILURE;
            }
        };
        target.entry = match relative.to_str() {
            Some(relative) => topaz_resolve::normalize_path(relative),
            None => {
                eprintln!(
                    "topaz: package test entry `{entry}` is not a Unicode path inside `{}`",
                    target.root.to_string_lossy()
                );
                return ExitCode::FAILURE;
            }
        };
        return match compiler_selection {
            CompilerSelection::Rust => test_package_target(&target, program_args),
            CompilerSelection::SelfHosted => run_self_package(&target, program_args, true),
        };
    }
    match compiler_selection {
        CompilerSelection::Rust => test_entry(entry, root, version, locked, program_args),
        CompilerSelection::SelfHosted => run_self_entry(entry, root, version, program_args, true),
    }
}

pub(super) fn finish_test_run(
    label: &str,
    unit: &topaz_resolve::ResolveOutput,
    host: &topaz_interp::TestHost,
    explicit_main: bool,
    outcome: Result<Value, topaz_interp::RtError>,
) -> ExitCode {
    for line in host.stdout() {
        println!("{line}");
    }
    let defer_errors = host.defer_errors();
    if !defer_errors.is_empty() {
        for err in defer_errors {
            eprintln!("topaz test defer error: {err}");
        }
        eprintln!("{label}: test failed (defer errors)");
        return ExitCode::FAILURE;
    }
    match outcome {
        Ok(value) => {
            let exit = explicit_main_exit(value, explicit_main);
            if exit == ExitCode::SUCCESS {
                println!("{label}: test-ok");
            } else {
                eprintln!("{label}: test failed");
            }
            exit
        }
        Err(e) => {
            let diag =
                Diagnostic::error(Code::new(e.code), e.message.clone(), Label::new(e.span, ""));
            eprintln!("{}", render(&diag, &unit.map));
            eprintln!("{label}: test failed");
            ExitCode::FAILURE
        }
    }
}

/// `topaz test <entry>` resolves, checks, and executes a v5.2+ module unit.
/// The v5.1 language has no module system or package test route.
pub(super) fn test_entry(
    entry: &str,
    root: Option<&str>,
    version: LangVersion,
    locked: bool,
    program_args: &[String],
) -> ExitCode {
    if locked {
        eprintln!("topaz: `--locked` applies only to package-mode test\n\n{USAGE}");
        return ExitCode::FAILURE;
    }
    if version == LangVersion::V5_1 {
        eprintln!(
            "topaz: `test` needs v5.2+ (v5.16 is the default); `--language-version 5.1` has no module system"
        );
        return ExitCode::FAILURE;
    }
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
    if let Err(n) = check_resolved_unit(&unit, false, version) {
        eprintln!(
            "{entry_norm}: {n} type diagnostic{}",
            if n == 1 { "" } else { "s" }
        );
        return ExitCode::FAILURE;
    }
    let host = cli_test_host();
    let invocation = ResolvedUnitInvocation::new(&unit);
    if let Err(code) = admit_cli_program_args(invocation.has_explicit_main(), program_args) {
        return code;
    }
    let outcome = invocation.run(&host, program_args);
    finish_test_run(
        &entry_norm,
        &unit,
        &host,
        invocation.has_explicit_main(),
        outcome,
    )
}
