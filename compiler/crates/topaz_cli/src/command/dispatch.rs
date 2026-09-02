use crate::*;

pub(super) fn dispatch_command(request: CommandDispatch<'_>) -> ExitCode {
    let CommandDispatch {
        args,
        root: root_arg,
        version_arg,
        current_version,
        standalone_version,
        locked: locked_flag,
        out_dir,
        observation_terminal,
        compiler_selection,
        preview_terminal,
        preview_producer,
        self_source: self_source_flag,
        comparison_layer,
        check_profile,
        json_format,
        types: types_flag,
        exports_json: exports_json_flag,
        unchecked: unchecked_flag,
        program_args,
        fmt_check: fmt_check_flag,
        emit_target,
        backend,
        backend_arg,
        native_report,
        release: release_flag,
        run: run_flag,
        build_target,
        experimental: experimental_flag,
        target: target_arg,
        from: from_arg,
        to: to_arg,
        path: path_arg,
        port: port_arg,
        json: json_flag,
        verbose: verbose_flag,
    } = request;
    match (args.first().map(String::as_str), args.len()) {
        (Some("parse"), 2) => parse_file(&args[1], false, standalone_version, compiler_selection),
        (Some("dump-ast"), 2) => parse_file(&args[1], true, standalone_version, compiler_selection),
        (Some("compiler"), 2) if args[1] == "status" => compiler_status(json_flag),
        (Some("compiler"), 2) if args[1] == "observe" => compiler_observe(CompilerObserveRequest {
            entry: None,
            root: root_arg,
            version_arg,
            version: current_version,
            locked: locked_flag,
            out_dir,
            terminal: observation_terminal,
            compiler_selection,
        }),
        (Some("compiler"), 3) if args[1] == "observe" => compiler_observe(CompilerObserveRequest {
            entry: Some(&args[2]),
            root: root_arg,
            version_arg,
            version: current_version,
            locked: locked_flag,
            out_dir,
            terminal: observation_terminal,
            compiler_selection,
        }),
        (Some("compiler"), 2) if args[1] == "preview" => compiler_preview(CompilerPreviewRequest {
            entry: None,
            root: root_arg,
            version_arg,
            version: current_version,
            locked: locked_flag,
            out_dir,
            terminal: preview_terminal,
            preview_producer,
            self_source: self_source_flag,
        }),
        (Some("compiler"), 3) if args[1] == "preview" => compiler_preview(CompilerPreviewRequest {
            entry: Some(&args[2]),
            root: root_arg,
            version_arg,
            version: current_version,
            locked: locked_flag,
            out_dir,
            terminal: preview_terminal,
            preview_producer,
            self_source: self_source_flag,
        }),
        (Some("compiler"), 3) if args[1] == "validate" => compiler_validate_observation(&args[2]),
        (Some("compiler"), 4) if args[1] == "compare" => {
            compiler_compare(&args[2], &args[3], comparison_layer)
        }
        (Some("check"), 1) => match if check_profile == Some(profile::CheckProfile::Bootstrap) {
            bootstrap_package_target(root_arg, version_arg, locked_flag)
        } else {
            package_target(root_arg, version_arg, locked_flag)
        } {
            Ok(target) => match check_profile {
                Some(profile) => check_package_target_with_profile(
                    &target,
                    profile,
                    json_format,
                    compiler_selection,
                ),
                None => check_package_target(
                    &target,
                    types_flag,
                    json_format,
                    exports_json_flag,
                    compiler_selection,
                ),
            },
            Err(code) => code,
        },
        (Some("check"), 2) => match check_profile {
            Some(profile) => check_unit_with_profile(
                &args[1],
                root_arg,
                current_version,
                profile,
                json_format,
                compiler_selection,
            ),
            None => check_unit(
                &args[1],
                root_arg,
                standalone_version,
                types_flag,
                json_format,
                exports_json_flag,
                compiler_selection,
            ),
        },
        (Some(cmd), _) if types_flag => {
            eprintln!(
                "topaz: `--types` applies to `check` only (got `{cmd}`)

{USAGE}"
            );
            ExitCode::FAILURE
        }
        (Some("run"), 1) => match package_target(root_arg, version_arg, locked_flag) {
            Ok(target) => match compiler_selection {
                CompilerSelection::Rust => {
                    run_package_target(&target, unchecked_flag, program_args)
                }
                CompilerSelection::SelfHosted => run_self_package(&target, program_args, false),
            },
            Err(code) => code,
        },
        (Some("run"), 2) => match compiler_selection {
            CompilerSelection::Rust => run_entry(
                &args[1],
                root_arg,
                standalone_version,
                unchecked_flag,
                program_args,
            ),
            CompilerSelection::SelfHosted => {
                run_self_entry(&args[1], root_arg, standalone_version, program_args, false)
            }
        },
        (Some("test"), 1) => match package_target(root_arg, version_arg, locked_flag) {
            Ok(target) => match compiler_selection {
                CompilerSelection::Rust => test_package_target(&target, program_args),
                CompilerSelection::SelfHosted => run_self_package(&target, program_args, true),
            },
            Err(code) => code,
        },
        (Some("test"), 2) => test_selected_entry(
            &args[1],
            root_arg,
            version_arg,
            standalone_version,
            locked_flag,
            program_args,
            compiler_selection,
        ),
        (Some("fmt"), 1) => fmt_package(root_arg, version_arg, fmt_check_flag, compiler_selection),
        (Some("fmt"), 2) => fmt_entry(
            &args[1],
            root_arg,
            standalone_version,
            fmt_check_flag,
            compiler_selection,
        ),
        (Some("lsp"), 1) => lsp_stdio(current_version, root_arg, compiler_selection),
        (Some("emit"), 1) => match package_target(root_arg, version_arg, locked_flag) {
            Ok(target) => match compiler_selection {
                CompilerSelection::Rust => emit_package_target(
                    &target,
                    out_dir,
                    unchecked_flag,
                    backend,
                    emit_target,
                    native_report.as_mut(),
                ),
                CompilerSelection::SelfHosted => {
                    emit_self_package_target(&target, out_dir, emit_target)
                }
            },
            Err(code) => code,
        },
        (Some("emit"), 2) => match compiler_selection {
            CompilerSelection::Rust => emit_entry(EmitEntryRequest {
                entry: &args[1],
                root: root_arg,
                out_dir,
                standalone_version,
                unchecked_flag,
                backend,
                emit_target,
                native_report: native_report.as_mut(),
            }),
            CompilerSelection::SelfHosted => {
                emit_self_entry(&args[1], root_arg, out_dir, standalone_version, emit_target)
            }
        },
        (Some("build"), 1) => match package_target(root_arg, version_arg, locked_flag) {
            Ok(target) => match compiler_selection {
                CompilerSelection::Rust => build_package_target(
                    &target,
                    out_dir.map(Path::new),
                    release_flag,
                    run_flag,
                    unchecked_flag,
                    backend,
                    backend_arg.is_some(),
                    build_target,
                    experimental_flag,
                    program_args,
                    native_report.as_mut(),
                ),
                CompilerSelection::SelfHosted => build_self_package_target(
                    &target,
                    out_dir.map(Path::new),
                    release_flag,
                    run_flag,
                    build_target,
                    program_args,
                ),
            },
            Err(code) => code,
        },
        (Some("build"), 2) => match compiler_selection {
            CompilerSelection::Rust => build_entry(
                &args[1],
                root_arg,
                out_dir,
                release_flag,
                run_flag,
                standalone_version,
                unchecked_flag,
                backend,
                build_target,
                experimental_flag,
                program_args,
                native_report.as_mut(),
            ),
            CompilerSelection::SelfHosted => build_self_entry(
                &args[1],
                root_arg,
                out_dir,
                release_flag,
                run_flag,
                standalone_version,
                build_target,
                program_args,
            ),
        },
        (Some("dev"), 1) => match package_target(root_arg, version_arg, locked_flag) {
            Ok(target) => dev_package_target(&target, out_dir, port_arg, compiler_selection),
            Err(code) => code,
        },
        (Some("init"), 1) => init_package(root_arg, version_arg, target_arg),
        (Some("add"), 2) => add_package_dependency(root_arg, version_arg, &args[1], path_arg),
        (Some("lock"), 1) => write_package_lock(root_arg, version_arg),
        (Some("fetch"), 1) => fetch_package(root_arg, version_arg, from_arg),
        (Some("vendor"), 1) => vendor_package(root_arg, version_arg, from_arg),
        (Some("doc"), 1) => match package_target(root_arg, version_arg, locked_flag) {
            Ok(target) => doc_package_target(&target, out_dir, compiler_selection),
            Err(code) => code,
        },
        (Some("refactor"), 4) if args[1] == "rename" => refactor_rename(
            root_arg,
            version_arg,
            &args[2],
            &args[3],
            None,
            standalone_version,
        ),
        (Some("refactor"), 5) if args[1] == "rename" => refactor_rename(
            root_arg,
            version_arg,
            &args[2],
            &args[3],
            Some(&args[4]),
            standalone_version,
        ),
        (Some("refactor"), 2) if args[1] == "organize-imports" => {
            refactor_organize_imports(root_arg, version_arg, None, standalone_version)
        }
        (Some("refactor"), 3) if args[1] == "organize-imports" => {
            refactor_organize_imports(root_arg, version_arg, Some(&args[2]), standalone_version)
        }
        (Some("refactor"), 2) if args[1] == "add-missing-match-cases" => {
            refactor_add_missing_match_cases(root_arg, version_arg, None, standalone_version)
        }
        (Some("refactor"), 3) if args[1] == "add-missing-match-cases" => {
            refactor_add_missing_match_cases(
                root_arg,
                version_arg,
                Some(&args[2]),
                standalone_version,
            )
        }
        (Some("refactor"), 3) if args[1] == "derive-json" => {
            refactor_derive_json(root_arg, &args[2], standalone_version)
        }
        (Some("migrate"), 1) => {
            migrate_package_or_entry(root_arg, version_arg, from_arg, to_arg, None)
        }
        (Some("migrate"), 2) => {
            migrate_package_or_entry(root_arg, version_arg, from_arg, to_arg, Some(&args[1]))
        }
        (Some("bench"), 1) => match package_target(root_arg, version_arg, locked_flag) {
            Ok(target) => match compiler_selection {
                CompilerSelection::Rust => bench_package_target(&target, json_flag),
                CompilerSelection::SelfHosted => {
                    let started = std::time::Instant::now();
                    bench_self_product(
                        compile_self_package_product(&target, None, "bench"),
                        &target.entry,
                        started,
                        json_flag,
                        CheckPresentation::Package,
                    )
                }
            },
            Err(code) => code,
        },
        (Some("bench"), 2) => match compiler_selection {
            CompilerSelection::Rust => {
                bench_entry(&args[1], root_arg, standalone_version, json_flag)
            }
            CompilerSelection::SelfHosted => {
                let started = std::time::Instant::now();
                bench_self_product(
                    compile_self_entry_product(
                        &args[1],
                        root_arg,
                        standalone_version,
                        None,
                        "bench",
                    ),
                    &args[1],
                    started,
                    json_flag,
                    CheckPresentation::Standalone,
                )
            }
        },
        (Some("storage"), 2) if args[1] == "status" => match storage::status() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("topaz: storage status failed: {e}");
                ExitCode::FAILURE
            }
        },
        (Some("storage"), 2) if args[1] == "clean" => match storage::clean() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("topaz: storage clean failed: {e}");
                ExitCode::FAILURE
            }
        },
        (Some("lispex"), 3) if args[1] == "run" => lispex::run_program(&args[2]),
        (Some("lispex"), 2) if args[1] == "info" && json_flag => lispex::info_json(),
        (Some("check-corpus"), 1) => check_corpus(),
        (Some("explain"), 2) => explain_diagnostic(&args[1], json_flag),
        (Some("version"), 1) => {
            print_version(verbose_flag, current_version);
            ExitCode::SUCCESS
        }
        (Some("license"), 1) => {
            print!("{}", artifact::license_text());
            ExitCode::SUCCESS
        }
        (Some("notice"), 1) => {
            print!("{}", artifact::notice_text());
            ExitCode::SUCCESS
        }
        (Some("help"), 1) | (None, _) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        (
            Some(
                cmd @ ("parse" | "dump-ast" | "check" | "check-corpus" | "run" | "emit" | "build"
                | "test" | "fmt" | "lsp" | "init" | "add" | "lock" | "fetch" | "vendor"
                | "doc" | "refactor" | "migrate" | "bench" | "explain" | "version"
                | "lispex" | "license" | "notice" | "help"),
            ),
            _,
        ) => {
            eprintln!("topaz: wrong arguments for `{cmd}`\n\n{USAGE}");
            ExitCode::FAILURE
        }
        (Some(other), _) => {
            eprintln!("topaz: unknown command `{other}`\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

pub(super) fn run_cli() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(exit) = dispatch_protocol_entry(&args) {
        return exit;
    }
    let program_separator_flag = args.iter().any(|arg| arg == "--");
    let program_args = take_program_args(&mut args);
    let version_arg = match take_language_version(&mut args) {
        Ok(v) => v,
        Err(bad) => {
            eprintln!(
                "topaz: invalid --language-version `{bad}`

{USAGE}"
            );
            return ExitCode::FAILURE;
        }
    };
    // The corpus harness is not version-bearing yet (CDR-002 Phase A
    // extends it with the v5.2 areas); reject the flag rather than
    // silently ignore it.
    if args.first().map(String::as_str) == Some("check-corpus") && version_arg.is_some() {
        eprintln!(
            "topaz: `check-corpus` does not take --language-version yet

{USAGE}"
        );
        return ExitCode::FAILURE;
    }
    // Product identity and unmarked single-file source identity are separate.
    // Package routes resolve their exact language from topaz.toml before
    // compiler selection. Current-only routes select CURRENT explicitly, while
    // ordinary standalone source remains pinned to UNMARKED_SOURCE.
    let current_version = version_arg.unwrap_or(LangVersion::CURRENT);
    let standalone_version = version_arg.unwrap_or(LangVersion::UNMARKED_SOURCE);
    let CommandFlags {
        compiler_intent,
        types: types_flag,
        release: release_flag,
        run: run_flag,
        unchecked: unchecked_flag,
        experimental: experimental_flag,
        locked: locked_flag,
        self_source: self_source_flag,
        fmt_check: fmt_check_flag,
        exports_json: exports_json_flag,
        json: json_flag,
        verbose: verbose_flag,
        version: version_flag,
        root: root_arg,
        out_dir,
        native_report: native_report_arg,
        comparison_layer_arg,
        comparison_layer,
        observation_terminal_arg,
        observation_terminal,
        preview_terminal,
        producer_arg,
        preview_producer,
        port: port_arg,
        from: from_arg,
        to: to_arg,
        path: path_arg,
        profile_arg,
        check_profile,
        format_arg,
        json_format,
        backend_arg,
        backend,
        target: target_arg,
    } = match CommandFlags::take(&mut args) {
        Ok(flags) => flags,
        Err(error) => {
            eprintln!("topaz: {error}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let command = args.first().map(String::as_str);
    if port_arg.is_some() && command != Some("dev") {
        eprintln!("topaz: `--port` applies to `dev` only\n\n{USAGE}");
        return ExitCode::FAILURE;
    }
    if fmt_check_flag && command != Some("fmt") {
        eprintln!("topaz: `--check` applies to `fmt` only\n\n{USAGE}");
        return ExitCode::FAILURE;
    }
    let emit_target = match command_emit_target(command, target_arg.as_deref()) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("topaz: {error}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let build_target = match command_build_target(command, target_arg.as_deref()) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("topaz: {error}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let default_package_build =
        command == Some("build") && args.len() == 1 && build_target == BuildTarget::Default;
    let compiler_target = match command {
        Some("emit") => Some(emit_target.label()),
        Some("build") => Some(build_target.label()),
        _ => None,
    };
    let current_only_invocation = check_profile.is_some()
        || command == Some("lsp")
        || matches!(args.as_slice(), [compiler, ..] if compiler == "compiler");
    let preflight_default_version = if current_only_invocation {
        current_version
    } else {
        standalone_version
    };
    let preflight_version = if version_flag {
        current_version
    } else {
        match compiler_preflight_language_version(
            &args,
            root_arg.as_deref(),
            version_arg,
            preflight_default_version,
            check_profile == Some(profile::CheckProfile::Bootstrap),
        ) {
            Ok(version) => version,
            Err(error) => {
                eprintln!("topaz: {error}");
                return ExitCode::FAILURE;
            }
        }
    };
    let resolved_compiler = match compiler_support::preflight(&PreflightRequest {
        intent: compiler_intent,
        product_default: CompilerSelection::Rust,
        args: &args,
        self_hosted_default_profile: preflight_version.uses_self_hosted_product_default(),
        locked: locked_flag,
        unchecked: unchecked_flag,
        experimental: experimental_flag,
        profile: check_profile.is_some(),
        exports_json: exports_json_flag,
        backend_native: backend == Backend::Native,
        native_report: native_report_arg.is_some(),
        producer: preview_producer.is_some(),
        self_source: self_source_flag,
        target: compiler_target,
    }) {
        Ok(selection) => selection,
        Err(error) => {
            eprintln!("topaz: {error}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    // Compiler-neutral commands intentionally resolve to no compiler. The
    // dispatch value is unreachable on those branches and stays Rust only to
    // avoid threading an irrelevant Option through unrelated product code.
    let compiler_selection = resolved_compiler
        .selected_compiler
        .unwrap_or(CompilerSelection::Rust);
    if resolved_compiler.selected_compiler.is_some()
        && INVOCATION_COMPILER_SELECTION
            .set(resolved_compiler)
            .is_err()
    {
        eprintln!("topaz: compiler selection was resolved more than once");
        return ExitCode::FAILURE;
    }
    if verbose_flag
        && let (Some(selected), Some(origin)) = (
            resolved_compiler.selected_compiler,
            resolved_compiler.selection_origin,
        )
    {
        eprintln!(
            "topaz: compiler selection: {} ({})",
            selected.selector(),
            origin.label()
        );
    }
    // `--unchecked` opts out of the default static check, for `run`/`emit`/`build`
    // only (CDR-003 §13). It is a usage error on every other command, on the bare
    // no-command case, AND alongside `--version`/`-V` (which carries no command verb)
    // — so it can never be silently ignored (§13.2). The guard runs AFTER the
    // value-flags (`--root`/`--out-dir`) are stripped, so `args.first()` is the true
    // command verb regardless of flag ordering (`topaz --root <dir> run --unchecked
    // <entry>` is a valid `run`, not a misuse), and BEFORE the `--version` early-return
    // below, so `topaz --unchecked --version` is rejected rather than silently ignored.
    if !validate_command(CommandValidation {
        args: &args,
        version_arg,
        current_version,
        types: types_flag,
        release: release_flag,
        run: run_flag,
        unchecked: unchecked_flag,
        experimental: experimental_flag,
        locked: locked_flag,
        self_source: self_source_flag,
        exports_json: exports_json_flag,
        json: json_flag,
        verbose: verbose_flag,
        version: version_flag,
        root: root_arg.as_ref(),
        out_dir: out_dir.as_ref(),
        native_report: native_report_arg.as_ref(),
        comparison_layer_arg: comparison_layer_arg.as_ref(),
        observation_terminal_arg: observation_terminal_arg.as_ref(),
        producer_arg: producer_arg.as_ref(),
        preview_producer,
        preview_terminal,
        from: from_arg.as_ref(),
        to: to_arg.as_ref(),
        path: path_arg.as_ref(),
        profile_arg: profile_arg.as_ref(),
        check_profile,
        format_arg: format_arg.as_ref(),
        json_format,
        backend_arg: backend_arg.as_ref(),
        backend,
        target: target_arg.as_ref(),
        emit_target,
        build_target,
        default_package_build,
        program_args: &program_args,
        program_separator: program_separator_flag,
    }) {
        return ExitCode::FAILURE;
    }
    // `--version`/`-V` is a flag, not a command: print and exit before dispatch.
    if version_flag {
        print_version(verbose_flag, current_version);
        return ExitCode::SUCCESS;
    }
    let mut native_report = match native_report_arg.as_deref() {
        Some(path) => match NativeReportSession::prepare(path, out_dir.as_deref()) {
            Ok(session) => Some(session),
            Err(error) => {
                eprintln!("topaz: cannot use native report path: {error}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let exit = dispatch_command(CommandDispatch {
        args: &args,
        root: root_arg.as_deref(),
        version_arg,
        current_version,
        standalone_version,
        locked: locked_flag,
        out_dir: out_dir.as_deref(),
        observation_terminal,
        compiler_selection,
        preview_terminal,
        preview_producer,
        self_source: self_source_flag,
        comparison_layer,
        check_profile,
        json_format,
        types: types_flag,
        exports_json: exports_json_flag,
        unchecked: unchecked_flag,
        program_args: &program_args,
        fmt_check: fmt_check_flag,
        emit_target,
        backend,
        backend_arg: backend_arg.as_deref(),
        native_report: &mut native_report,
        release: release_flag,
        run: run_flag,
        build_target,
        experimental: experimental_flag,
        target: target_arg.as_deref(),
        from: from_arg.as_deref(),
        to: to_arg.as_deref(),
        path: path_arg.as_deref(),
        port: port_arg.as_deref(),
        json: json_flag,
        verbose: verbose_flag,
    });
    if exit == ExitCode::SUCCESS
        && let Some(report) = native_report.as_mut()
        && let Err(error) = report.finish()
    {
        eprintln!("topaz: cannot write native lowering report: {error}");
        return ExitCode::FAILURE;
    }
    exit
}

pub(super) fn explain_diagnostic(code: &str, json: bool) -> ExitCode {
    if !is_explain_code_shape(code) {
        eprintln!("topaz: diagnostic code must have shape TPZ#### (got `{code}`)");
        return ExitCode::FAILURE;
    }
    let Some(explanation) = explain_code(code) else {
        eprintln!("topaz: no explanation registered for `{code}`");
        return ExitCode::FAILURE;
    };
    if json {
        println!("{}", render_explain_json(explanation));
    } else {
        print!("{}", render_explain(explanation));
    }
    ExitCode::SUCCESS
}

/// `topaz --version [--verbose]` / `topaz version`: the toolchain version.
/// Short form reports the exact product semver; verbose adds the
/// exact compiler/runtime artifact version, the language mode, and the backend.
/// The numbers come from `CARGO_PKG_VERSION`, so they track the workspace toolchain
/// version. The selected language mode is reported only in verbose output.
pub(super) fn print_version(verbose: bool, lang: LangVersion) {
    let mode = lang.as_str();
    if verbose {
        println!("Topaz compiler {}", env!("CARGO_PKG_VERSION"));
        println!("Language mode: topaz-{mode}");
        println!("Runtime: topaz_rt {}", env!("CARGO_PKG_VERSION"));
        println!("Rust backend: rust (CDR-006)");
    } else {
        println!("Topaz {}", env!("CARGO_PKG_VERSION"));
    }
}

/// Extracts `--language-version <v>` (or `--language-version=<v>`)
/// from `args`; `None` when the flag is absent (the session default
/// is the current line — CDR-007 §1).
pub(super) fn take_language_version(args: &mut Vec<String>) -> Result<Option<LangVersion>, String> {
    let mut version = None;
    let mut i = 0;
    while i < args.len() {
        let (is_flag, inline) = match args[i].as_str() {
            "--language-version" => (true, None),
            s if s.starts_with("--language-version=") => {
                (true, Some(s["--language-version=".len()..].to_string()))
            }
            _ => (false, None),
        };
        if !is_flag {
            i += 1;
            continue;
        }
        let value = match inline {
            Some(v) => {
                args.remove(i);
                v
            }
            None => {
                args.remove(i);
                if i < args.len() {
                    args.remove(i)
                } else {
                    return Err(String::new());
                }
            }
        };
        version = Some(LangVersion::parse_selectable(&value).ok_or_else(|| value.to_string())?);
    }
    Ok(version)
}

pub(super) fn corpus_fixture_language_version(value: &str) -> Option<LangVersion> {
    if value == "both" {
        return Some(LangVersion::V5_2);
    }
    LangVersion::parse_exact(value).filter(|version| *version <= LangVersion::V5_7)
}

/// Extracts a `--flag <value>` (or `--flag=<value>`) pair from `args`.
/// Removes a boolean flag from the argument list, reporting whether
/// it was present.
pub(super) fn take_bool_flag(args: &mut Vec<String>, flag: &str) -> bool {
    if let Some(pos) = args.iter().position(|a| a == flag) {
        args.remove(pos);
        true
    } else {
        false
    }
}

pub(super) fn take_program_args(args: &mut Vec<String>) -> Vec<String> {
    let Some(pos) = args.iter().position(|arg| arg == "--") else {
        return Vec::new();
    };
    let program_args = args.split_off(pos + 1);
    args.pop();
    program_args
}

pub(super) fn take_value_flag(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, ()> {
    let mut value = None;
    let mut i = 0;
    let inline_prefix = format!("{flag}=");
    while i < args.len() {
        if args[i] == flag {
            args.remove(i);
            if i < args.len() {
                value = Some(args.remove(i));
            } else {
                return Err(());
            }
        } else if let Some(v) = args[i].strip_prefix(&inline_prefix) {
            value = Some(v.to_string());
            args.remove(i);
        } else {
            i += 1;
        }
    }
    Ok(value)
}
