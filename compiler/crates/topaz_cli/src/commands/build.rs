use crate::*;

pub(super) fn build_self_package_target(
    target: &PackageTarget,
    out_dir: Option<&Path>,
    release: bool,
    run: bool,
    build_target: BuildTarget,
    program_args: &[String],
) -> ExitCode {
    let build_target = if build_target == BuildTarget::Default {
        match manifest_build_target(&target.build_target) {
            Ok(target) => target,
            Err(error) => {
                eprintln!("topaz: {error}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        build_target
    };
    let Some(dir) = out_dir.filter(|dir| !dir.as_os_str().is_empty()) else {
        eprintln!("topaz: `build` requires `--out-dir <dir>` (a non-empty directory)");
        return ExitCode::FAILURE;
    };
    let product = match compile_self_package_product(target, None, "build") {
        Ok(product) => product,
        Err(code) => return code,
    };
    let lispex_application = match self_lispex_application_plan(target, &product) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    if build_target.is_python() {
        if let Err(code) =
            reject_self_lispex_application_target(lispex_application.as_ref(), "python")
        {
            return code;
        }
        let generated = match completed_self_python_source(
            product,
            &target.entry,
            CheckPresentation::Package,
        ) {
            Ok(generated) => generated,
            Err(code) => return code,
        };
        let destination = match artifact::Destination::open(dir, artifact::Target::Python) {
            Ok(destination) => destination,
            Err(error) => {
                eprintln!("topaz: cannot use output directory: {error}");
                return ExitCode::FAILURE;
            }
        };
        return install_self_python_build(
            destination,
            dir,
            &target.entry,
            target.version,
            &generated.text,
            generated.compiler,
        );
    }
    if build_target.is_web() {
        if let Err(code) =
            reject_self_lispex_application_target(lispex_application.as_ref(), build_target.label())
        {
            return code;
        }
        let lowered = match lower_self_product_for_web(product) {
            Ok(lowered) => lowered,
            Err(code) => return code,
        };
        if build_target == BuildTarget::WebApp
            && let Err(error) = validate_web_app_lifecycle(&lowered, target.web.lifecycle)
        {
            eprintln!("topaz: invalid web-app lifecycle: {error}");
            return ExitCode::FAILURE;
        }
        return build_web_package(WebPackageBuild {
            dir,
            rust: &lowered.rust,
            compiler: lowered.compiler,
            release,
            label: &target.entry,
            entry_exports: &lowered.entry_exports,
            records: &lowered.records,
            enums: &lowered.enums,
            newtypes: &lowered.newtypes,
            language_version: target.version,
            target: build_target,
            package_root: (build_target == BuildTarget::WebApp).then_some(target.root.as_path()),
            package_name: (build_target == BuildTarget::WebApp)
                .then_some(target.package_name.as_str()),
            web: (build_target == BuildTarget::WebApp).then_some(&target.web),
            web_capabilities: (build_target == BuildTarget::WebApp)
                .then_some(&target.web_capabilities),
        });
    }
    if build_target.is_service() {
        if let Err(code) =
            reject_self_lispex_application_target(lispex_application.as_ref(), build_target.label())
        {
            return code;
        }
        let lowered = match lower_self_product_for_service(product) {
            Ok(lowered) => lowered,
            Err(code) => return code,
        };
        if let Err(error) = validate_http_service_handler(&lowered) {
            eprintln!("topaz: invalid http-service handler: {error}");
            return ExitCode::FAILURE;
        }
        return build_http_service_artifact(
            dir,
            &target.entry,
            target.version,
            &lowered.rust,
            lowered.compiler,
            package_harness(target),
            &target.service,
            release,
        );
    }
    let generated =
        match completed_self_generated_source(product, &target.entry, CheckPresentation::Package) {
            Ok(generated) => generated,
            Err(code) => return code,
        };
    build_native_artifact(
        dir,
        &target.entry,
        target.version,
        &generated.text,
        generated.compiler,
        package_harness_with_lispex(target, lispex_application.as_ref()),
        generated.explicit_main,
        release,
        run,
        program_args,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_self_entry(
    entry: &str,
    root: Option<&str>,
    out_dir: Option<&str>,
    release: bool,
    run: bool,
    version: LangVersion,
    build_target: BuildTarget,
    program_args: &[String],
) -> ExitCode {
    let build_target = if build_target == BuildTarget::Default {
        BuildTarget::Native
    } else {
        build_target
    };
    if build_target.is_service() || build_target == BuildTarget::WebApp {
        eprintln!(
            "topaz: self `{}` route requires a package target or is not complete; recovery: rerun with `--compiler rust` (not executed)",
            build_target.label()
        );
        return ExitCode::FAILURE;
    }
    let Some(dir) = out_dir.filter(|dir| !dir.is_empty()) else {
        eprintln!("topaz: `build` requires `--out-dir <dir>` (a non-empty directory)");
        return ExitCode::FAILURE;
    };
    let product = match compile_self_entry_product(entry, root, version, None, "build") {
        Ok(product) => product,
        Err(code) => return code,
    };
    if build_target.is_python() {
        let generated =
            match completed_self_python_source(product, entry, CheckPresentation::Standalone) {
                Ok(generated) => generated,
                Err(code) => return code,
            };
        let destination =
            match artifact::Destination::open(Path::new(dir), artifact::Target::Python) {
                Ok(destination) => destination,
                Err(error) => {
                    eprintln!("topaz: cannot use output directory: {error}");
                    return ExitCode::FAILURE;
                }
            };
        return install_self_python_build(
            destination,
            Path::new(dir),
            entry,
            version,
            &generated.text,
            generated.compiler,
        );
    }
    if build_target.is_web() {
        let lowered = match lower_self_product_for_web(product) {
            Ok(lowered) => lowered,
            Err(code) => return code,
        };
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
    let generated =
        match completed_self_generated_source(product, entry, CheckPresentation::Standalone) {
            Ok(generated) => generated,
            Err(code) => return code,
        };
    build_native_artifact(
        Path::new(dir),
        entry,
        version,
        &generated.text,
        generated.compiler,
        HostHarness::Unrestricted,
        generated.explicit_main,
        release,
        run,
        program_args,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_package_target(
    target: &PackageTarget,
    out_dir: Option<&Path>,
    release: bool,
    run: bool,
    unchecked: bool,
    backend: Backend,
    backend_selected: bool,
    build_target: BuildTarget,
    experimental: bool,
    program_args: &[String],
    mut native_report: Option<&mut NativeReportSession>,
) -> ExitCode {
    let build_target = if build_target == BuildTarget::Default {
        match manifest_build_target(&target.build_target) {
            Ok(target) => target,
            Err(error) => {
                eprintln!("topaz: {error}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        build_target
    };
    if build_target.is_web() && run {
        eprintln!(
            "topaz: `build --target {}` cannot be combined with `--run`\n\n{USAGE}",
            build_target.label()
        );
        return ExitCode::FAILURE;
    }
    if build_target.is_python() && run {
        eprintln!("topaz: `build --target python` cannot be combined with `--run`\n\n{USAGE}");
        return ExitCode::FAILURE;
    }
    if build_target.is_service() && run {
        eprintln!(
            "topaz: `build --target http-service` cannot be combined with `--run`; start the generated service explicitly\n\n{USAGE}"
        );
        return ExitCode::FAILURE;
    }
    if build_target.is_python() && release {
        eprintln!("topaz: `--release` does not apply to `build --target python`\n\n{USAGE}");
        return ExitCode::FAILURE;
    }
    if build_target.is_web() && unchecked {
        eprintln!(
            "topaz: `build --target {}` requires the checked build (it emits a typed TS facade); \
             drop `--unchecked`\n\n{USAGE}",
            build_target.label()
        );
        return ExitCode::FAILURE;
    }
    if build_target.is_service() && unchecked {
        eprintln!(
            "topaz: `build --target http-service` requires the checked build; drop `--unchecked`\n\n{USAGE}"
        );
        return ExitCode::FAILURE;
    }
    if build_target.is_service() && backend_selected && backend == Backend::Native {
        eprintln!(
            "topaz: `build --target http-service` currently requires `--backend boxed` so handler deadlines retain cooperative checkpoints\n\n{USAGE}"
        );
        return ExitCode::FAILURE;
    }
    if build_target.is_python() && backend_selected {
        eprintln!(
            "topaz: `--backend` applies to Rust targets; drop it for `--target python`\n\n{USAGE}"
        );
        return ExitCode::FAILURE;
    }
    if experimental && !build_target.is_python() {
        eprintln!(
            "topaz: legacy `--experimental` applies only to `build --target python`\n\n{USAGE}"
        );
        return ExitCode::FAILURE;
    }
    let dir = match out_dir {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        Some(_) | None => {
            eprintln!("topaz: `build` requires `--out-dir <dir>` (a non-empty directory)");
            return ExitCode::FAILURE;
        }
    };
    if experimental {
        eprintln!(
            "topaz: warning: `--experimental` is deprecated; Python is a regular deployment target in v5.9"
        );
    }
    if build_target.is_python() {
        let generated = match resolve_and_emit_python_application_package(target, unchecked) {
            Ok(generated) => generated,
            Err(code) => return code,
        };
        let destination = match artifact::Destination::open(dir, artifact::Target::Python) {
            Ok(destination) => destination,
            Err(e) => {
                eprintln!("topaz: cannot use output directory: {e}");
                return ExitCode::FAILURE;
            }
        };
        return install_python_build(
            destination,
            dir,
            &target.entry,
            target.version,
            &generated.text,
            generated.compiler,
        );
    }
    if build_target.is_web() {
        let lowered = match resolve_and_lower_package_for_web_with_report(
            target,
            backend,
            native_report.as_deref_mut(),
            "build",
            build_target.label(),
        ) {
            Ok(lowered) => lowered,
            Err(code) => return code,
        };
        if build_target == BuildTarget::WebApp
            && let Err(error) = validate_web_app_lifecycle(&lowered, target.web.lifecycle)
        {
            eprintln!("topaz: invalid web-app lifecycle: {error}");
            return ExitCode::FAILURE;
        }
        return build_web_package(WebPackageBuild {
            dir,
            rust: &lowered.rust,
            compiler: lowered.compiler,
            release,
            label: &target.entry,
            entry_exports: &lowered.entry_exports,
            records: &lowered.records,
            enums: &lowered.enums,
            newtypes: &lowered.newtypes,
            language_version: target.version,
            target: build_target,
            package_root: (build_target == BuildTarget::WebApp).then_some(target.root.as_path()),
            package_name: (build_target == BuildTarget::WebApp)
                .then_some(target.package_name.as_str()),
            web: (build_target == BuildTarget::WebApp).then_some(&target.web),
            web_capabilities: (build_target == BuildTarget::WebApp)
                .then_some(&target.web_capabilities),
        });
    }
    if build_target.is_service() {
        let lowered = match resolve_and_lower_package_for_service(target) {
            Ok(lowered) => lowered,
            Err(code) => return code,
        };
        if let Err(error) = validate_http_service_handler(&lowered) {
            eprintln!("topaz: invalid http-service handler: {error}");
            return ExitCode::FAILURE;
        }
        return build_http_service_artifact(
            dir,
            &target.entry,
            target.version,
            &lowered.rust,
            lowered.compiler,
            package_harness(target),
            &target.service,
            release,
        );
    }
    let rust = match resolve_and_lower_package_with_report(
        target,
        unchecked,
        backend,
        native_report,
        "build",
        build_target.label(),
    ) {
        Ok(rust) => rust,
        Err(code) => return code,
    };
    build_native_artifact(
        dir,
        &target.entry,
        target.version,
        &rust.text,
        rust.compiler,
        package_harness_with_lispex(target, rust.lispex_application.as_ref()),
        rust.explicit_main,
        release,
        run,
        program_args,
    )
}
