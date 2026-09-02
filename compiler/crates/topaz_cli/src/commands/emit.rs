use crate::*;

pub(super) fn emit_generated_rust(
    out_dir: Option<&str>,
    generated: &GeneratedSource,
    harness: HostHarness<'_>,
) -> ExitCode {
    match out_dir {
        None => {
            print!("{}", generated.text);
            ExitCode::SUCCESS
        }
        Some("") => {
            eprintln!("topaz: `--out-dir` requires a non-empty directory");
            ExitCode::FAILURE
        }
        Some(dir) => {
            if let Err(error) = scaffold_crate(Path::new(dir), &generated.text, harness) {
                eprintln!("topaz: could not write the crate to `{dir}`: {error}");
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

pub(super) fn emit_self_package_target(
    target: &PackageTarget,
    out_dir: Option<&str>,
    emit_target: EmitTarget,
) -> ExitCode {
    let product = match compile_self_package_product(target, None, "emit") {
        Ok(product) => product,
        Err(code) => return code,
    };
    let lispex_application = match self_lispex_application_plan(target, &product) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    if emit_target.is_python() {
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
        return emit_self_python_source(out_dir, &generated.text);
    }
    let generated =
        match completed_self_generated_source(product, &target.entry, CheckPresentation::Package) {
            Ok(generated) => generated,
            Err(code) => return code,
        };
    emit_generated_rust(
        out_dir,
        &generated,
        package_harness_with_lispex(target, lispex_application.as_ref()),
    )
}

pub(super) fn emit_self_entry(
    entry: &str,
    root: Option<&str>,
    out_dir: Option<&str>,
    version: LangVersion,
    emit_target: EmitTarget,
) -> ExitCode {
    let product = match compile_self_entry_product(entry, root, version, None, "emit") {
        Ok(product) => product,
        Err(code) => return code,
    };
    if emit_target.is_python() {
        let generated =
            match completed_self_python_source(product, entry, CheckPresentation::Standalone) {
                Ok(generated) => generated,
                Err(code) => return code,
            };
        return emit_self_python_source(out_dir, &generated.text);
    }
    let generated =
        match completed_self_generated_source(product, entry, CheckPresentation::Standalone) {
            Ok(generated) => generated,
            Err(code) => return code,
        };
    emit_generated_rust(out_dir, &generated, HostHarness::Unrestricted)
}

pub(super) fn emit_package_target(
    target: &PackageTarget,
    out_dir: Option<&str>,
    unchecked: bool,
    backend: Backend,
    emit_target: EmitTarget,
    native_report: Option<&mut NativeReportSession>,
) -> ExitCode {
    if emit_target.is_python() {
        let generated = match resolve_and_emit_python_package(target, unchecked) {
            Ok(generated) => generated,
            Err(code) => return code,
        };
        return emit_python_source(out_dir, &generated.text);
    }
    let rust = match resolve_and_lower_package_with_report(
        target,
        unchecked,
        backend,
        native_report,
        "emit",
        "rust",
    ) {
        Ok(rust) => rust,
        Err(code) => return code,
    };
    match out_dir {
        None => {
            if rust
                .lispex_application
                .as_ref()
                .is_some_and(|plan| !plan.reachable_rules.is_empty())
            {
                eprintln!(
                    "topaz: a reached Lispex application requires `emit --out-dir` so its exact conditional runtime closure can be written"
                );
                return ExitCode::FAILURE;
            }
            print!("{}", rust.text);
            ExitCode::SUCCESS
        }
        Some("") => {
            eprintln!("topaz: `--out-dir` requires a non-empty directory");
            ExitCode::FAILURE
        }
        Some(dir) => {
            let harness = package_harness_with_lispex(target, rust.lispex_application.as_ref());
            if let Err(e) = scaffold_crate(Path::new(dir), &rust.text, harness) {
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
