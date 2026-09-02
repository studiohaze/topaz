use crate::*;

pub(super) struct CommandDispatch<'scope> {
    pub(super) args: &'scope [String],
    pub(super) root: Option<&'scope str>,
    pub(super) version_arg: Option<LangVersion>,
    pub(super) current_version: LangVersion,
    pub(super) standalone_version: LangVersion,
    pub(super) locked: bool,
    pub(super) out_dir: Option<&'scope str>,
    pub(super) observation_terminal: topaz_kernel::TerminalPhase,
    pub(super) compiler_selection: CompilerSelection,
    pub(super) preview_terminal: topaz_kernel::TerminalPhase,
    pub(super) preview_producer: Option<PreviewProducer>,
    pub(super) self_source: bool,
    pub(super) comparison_layer: topaz_kernel::ComparisonLayer,
    pub(super) check_profile: Option<profile::CheckProfile>,
    pub(super) json_format: bool,
    pub(super) types: bool,
    pub(super) exports_json: bool,
    pub(super) unchecked: bool,
    pub(super) program_args: &'scope [String],
    pub(super) fmt_check: bool,
    pub(super) emit_target: EmitTarget,
    pub(super) backend: Backend,
    pub(super) backend_arg: Option<&'scope str>,
    pub(super) native_report: &'scope mut Option<NativeReportSession>,
    pub(super) release: bool,
    pub(super) run: bool,
    pub(super) build_target: BuildTarget,
    pub(super) experimental: bool,
    pub(super) target: Option<&'scope str>,
    pub(super) from: Option<&'scope str>,
    pub(super) to: Option<&'scope str>,
    pub(super) path: Option<&'scope str>,
    pub(super) port: Option<&'scope str>,
    pub(super) json: bool,
    pub(super) verbose: bool,
}

pub(super) struct CommandValidation<'scope> {
    pub(super) args: &'scope [String],
    pub(super) version_arg: Option<LangVersion>,
    pub(super) current_version: LangVersion,
    pub(super) types: bool,
    pub(super) release: bool,
    pub(super) run: bool,
    pub(super) unchecked: bool,
    pub(super) experimental: bool,
    pub(super) locked: bool,
    pub(super) self_source: bool,
    pub(super) exports_json: bool,
    pub(super) json: bool,
    pub(super) verbose: bool,
    pub(super) version: bool,
    pub(super) root: Option<&'scope String>,
    pub(super) out_dir: Option<&'scope String>,
    pub(super) native_report: Option<&'scope String>,
    pub(super) comparison_layer_arg: Option<&'scope String>,
    pub(super) observation_terminal_arg: Option<&'scope String>,
    pub(super) producer_arg: Option<&'scope String>,
    pub(super) preview_producer: Option<PreviewProducer>,
    pub(super) preview_terminal: topaz_kernel::TerminalPhase,
    pub(super) from: Option<&'scope String>,
    pub(super) to: Option<&'scope String>,
    pub(super) path: Option<&'scope String>,
    pub(super) profile_arg: Option<&'scope String>,
    pub(super) check_profile: Option<profile::CheckProfile>,
    pub(super) format_arg: Option<&'scope String>,
    pub(super) json_format: bool,
    pub(super) backend_arg: Option<&'scope String>,
    pub(super) backend: Backend,
    pub(super) target: Option<&'scope String>,
    pub(super) emit_target: EmitTarget,
    pub(super) build_target: BuildTarget,
    pub(super) default_package_build: bool,
    pub(super) program_args: &'scope [String],
    pub(super) program_separator: bool,
}

pub(super) fn validate_command(validation: CommandValidation<'_>) -> bool {
    let CommandValidation {
        args,
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
        root: root_arg,
        out_dir,
        native_report: native_report_arg,
        comparison_layer_arg,
        observation_terminal_arg,
        producer_arg,
        preview_producer,
        preview_terminal,
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
        emit_target,
        build_target,
        default_package_build,
        program_args,
        program_separator: program_separator_flag,
    } = validation;
    let command = args.first().map(String::as_str);
    if unchecked_flag
        && !matches!(
            args.first().map(String::as_str),
            Some("run") | Some("emit") | Some("build")
        )
    {
        eprintln!("topaz: `--unchecked` applies to run/emit/build only\n\n{USAGE}");
        return false;
    }
    // `--format json` applies to `check` only in v1 (the other commands keep the
    // human renderer); rejected elsewhere so it can never be silently ignored.
    if json_format && args.first().map(String::as_str) != Some("check") {
        eprintln!("topaz: `--format json` applies to `check` only\n\n{USAGE}");
        return false;
    }
    if exports_json_flag && args.first().map(String::as_str) != Some("check") {
        eprintln!("topaz: `--exports-json` applies to `check` only\n\n{USAGE}");
        return false;
    }
    if check_profile.is_some() && args.first().map(String::as_str) != Some("check") {
        eprintln!("topaz: `--profile` applies to `check` only\n\n{USAGE}");
        return false;
    }
    if check_profile.is_some() && args.len() == 2 && current_version != LangVersion::CURRENT {
        eprintln!(
            "topaz: `--profile` requires canonical language `topaz-{}` (got `topaz-{}`)\n\n{USAGE}",
            LangVersion::CURRENT.as_str(),
            lang_version_text(current_version)
        );
        return false;
    }
    if check_profile.is_some() && exports_json_flag {
        eprintln!("topaz: `--profile` cannot be combined with `--exports-json`\n\n{USAGE}");
        return false;
    }
    let lispex_info_json = args == ["lispex", "info"] && json_flag;
    let compiler_status_json = args == ["compiler", "status"] && json_flag;
    if json_flag
        && !matches!(
            args.first().map(String::as_str),
            Some("explain") | Some("bench")
        )
        && !lispex_info_json
        && !compiler_status_json
    {
        eprintln!(
            "topaz: `--json` applies to `compiler status`, `explain`, `bench`, and `lispex info` only\n\n{USAGE}"
        );
        return false;
    }
    if from_arg.is_some()
        && !matches!(
            args.first().map(String::as_str),
            Some("fetch") | Some("vendor") | Some("migrate")
        )
    {
        eprintln!("topaz: `--from` applies to `fetch`, `vendor`, and `migrate` only\n\n{USAGE}");
        return false;
    }
    if to_arg.is_some() && args.first().map(String::as_str) != Some("migrate") {
        eprintln!("topaz: `--to` applies to `migrate` only\n\n{USAGE}");
        return false;
    }
    if path_arg.is_some() && args.first().map(String::as_str) != Some("add") {
        eprintln!("topaz: `--path` applies to `add` only\n\n{USAGE}");
        return false;
    }
    if target_arg.is_some() && !matches!(command, Some("emit" | "build" | "init")) {
        eprintln!("topaz: `--target` applies to `emit`, `build`, or `init` only\n\n{USAGE}");
        return false;
    }
    if comparison_layer_arg.is_some()
        && !(args.first().map(String::as_str) == Some("compiler")
            && args.get(1).map(String::as_str) == Some("compare"))
    {
        eprintln!("topaz: `--layer` applies to `compiler compare` only\n\n{USAGE}");
        return false;
    }
    if observation_terminal_arg.is_some()
        && !(args.first().map(String::as_str) == Some("compiler")
            && matches!(args.get(1).map(String::as_str), Some("observe" | "preview")))
    {
        eprintln!(
            "topaz: `--terminal` applies to `compiler observe` or `compiler preview` only\n\n{USAGE}"
        );
        return false;
    }
    if producer_arg.is_some()
        && !(args.first().map(String::as_str) == Some("compiler")
            && args.get(1).map(String::as_str) == Some("preview"))
    {
        eprintln!("topaz: `--producer` applies to `compiler preview` only\n\n{USAGE}");
        return false;
    }
    if preview_producer.is_some() && preview_terminal != topaz_kernel::TerminalPhase::RustSource {
        eprintln!("topaz: `--producer` requires `--terminal rust-source`\n\n{USAGE}");
        return false;
    }
    if self_source_flag
        && !(args.first().map(String::as_str) == Some("compiler")
            && args.get(1).map(String::as_str) == Some("preview")
            && preview_producer == Some(PreviewProducer::Stage2)
            && preview_terminal == topaz_kernel::TerminalPhase::RustSource)
    {
        eprintln!(
            "topaz: `--self-source` requires `compiler preview --producer stage2 --terminal rust-source`\n\n{USAGE}"
        );
        return false;
    }
    if release_flag && command != Some("build") {
        eprintln!("topaz: `--release` applies to `build` only\n\n{USAGE}");
        return false;
    }
    if run_flag && command != Some("build") {
        eprintln!("topaz: `--run` applies to `build` only\n\n{USAGE}");
        return false;
    }
    if experimental_flag
        && !(args.first().map(String::as_str) == Some("build")
            && (build_target.is_python() || default_package_build))
    {
        eprintln!(
            "topaz: legacy `--experimental` applies only to `build --target python`\n\n{USAGE}"
        );
        return false;
    }
    if build_target.is_web() && run_flag {
        eprintln!(
            "topaz: `build --target {}` cannot be combined with `--run`\n\n{USAGE}",
            build_target.label()
        );
        return false;
    }
    if build_target.is_python() && run_flag {
        eprintln!("topaz: `build --target python` cannot be combined with `--run`\n\n{USAGE}");
        return false;
    }
    if build_target.is_service() && run_flag {
        eprintln!(
            "topaz: `build --target http-service` cannot be combined with `--run`; start the generated service explicitly\n\n{USAGE}"
        );
        return false;
    }
    if build_target.is_python() && release_flag {
        eprintln!("topaz: `--release` does not apply to `build --target python`\n\n{USAGE}");
        return false;
    }
    if build_target.is_web() && unchecked_flag {
        eprintln!(
            "topaz: `build --target {}` requires the checked build (it emits a typed TS facade); \
             drop `--unchecked`\n\n{USAGE}",
            build_target.label()
        );
        return false;
    }
    if build_target.is_service() && unchecked_flag {
        eprintln!(
            "topaz: `build --target http-service` requires the checked build; drop `--unchecked`\n\n{USAGE}"
        );
        return false;
    }
    if build_target.is_service() && backend == Backend::Native {
        eprintln!(
            "topaz: `build --target http-service` currently requires `--backend boxed` so handler deadlines retain cooperative checkpoints\n\n{USAGE}"
        );
        return false;
    }
    // `--backend native` applies to `emit`/`build` only, and requires the default
    // (checked) gate — the native backend consumes the typed HIR a clean check
    // produces, so `--unchecked` is always boxed. Reject the misuse rather than
    // silently ignore it.
    if backend == Backend::Native {
        if !matches!(
            args.first().map(String::as_str),
            Some("emit") | Some("build")
        ) {
            eprintln!("topaz: `--backend native` applies to emit/build only\n\n{USAGE}");
            return false;
        }
        if unchecked_flag {
            eprintln!(
                "topaz: `--backend native` requires the checked build (it consumes the typed HIR); \
                 drop `--unchecked`\n\n{USAGE}"
            );
            return false;
        }
    }
    if native_report_arg.is_some() {
        if !matches!(command, Some("emit" | "build")) {
            eprintln!("topaz: `--native-report-json` applies to emit/build only\n\n{USAGE}");
            return false;
        }
        if backend != Backend::Native {
            eprintln!("topaz: `--native-report-json` requires `--backend native`\n\n{USAGE}");
            return false;
        }
        if (command == Some("emit") && emit_target.is_python())
            || (command == Some("build") && build_target.is_python())
        {
            eprintln!("topaz: `--native-report-json` applies to Rust targets only\n\n{USAGE}");
            return false;
        }
    }
    if ((command == Some("build") && build_target.is_python())
        || (command == Some("emit") && emit_target.is_python()))
        && backend_arg.is_some()
    {
        eprintln!(
            "topaz: `--backend` applies to Rust targets; drop it for `--target python`\n\n{USAGE}"
        );
        return false;
    }
    let selected_package_test = matches!(
        (args.first().map(String::as_str), args.len()),
        (Some("test"), 2)
    ) && root_arg
        .is_some_and(|root| Path::new(root).join("topaz.toml").is_file());
    if locked_flag
        && !selected_package_test
        && !matches!(
            (args.first().map(String::as_str), args.len()),
            (
                Some("check" | "run" | "emit" | "build" | "test" | "doc" | "bench" | "dev"),
                1
            )
        )
        && args != ["compiler", "observe"]
        && args != ["compiler", "preview"]
    {
        eprintln!(
            "topaz: `--locked` applies only to package-mode check/run/emit/build/test/doc/bench/dev\n\n{USAGE}"
        );
        return false;
    }
    if !program_args.is_empty() {
        match (args.first().map(String::as_str), run_flag) {
            (Some("run" | "test"), _) | (Some("build"), true) => {}
            (Some("build"), false) => {
                eprintln!("topaz: `--` program args require `build --run`\n\n{USAGE}");
                return false;
            }
            _ => {
                eprintln!(
                    "topaz: `--` program args apply to `run`, `test`, and `build --run` only\n\n{USAGE}"
                );
                return false;
            }
        }
    }
    if command == Some("lispex")
        && (version_arg.is_some()
            || types_flag
            || release_flag
            || run_flag
            || unchecked_flag
            || experimental_flag
            || locked_flag
            || exports_json_flag
            || verbose_flag
            || version_flag
            || root_arg.is_some()
            || out_dir.is_some()
            || native_report_arg.is_some()
            || comparison_layer_arg.is_some()
            || from_arg.is_some()
            || to_arg.is_some()
            || path_arg.is_some()
            || profile_arg.is_some()
            || format_arg.is_some()
            || backend_arg.is_some()
            || target_arg.is_some()
            || program_separator_flag
            || (json_flag && !lispex_info_json))
    {
        eprintln!("topaz: `lispex` accepts only `run <file>` or `info --json`\n\n{USAGE}");
        return false;
    }
    true
}
