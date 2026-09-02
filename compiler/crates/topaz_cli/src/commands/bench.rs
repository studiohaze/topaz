use crate::*;

pub(super) fn bench_package_target(target: &PackageTarget, json: bool) -> ExitCode {
    let start = std::time::Instant::now();
    let out = resolve_package_target(target);
    for diag in &out.diagnostics {
        eprintln!("{}", render(diag, &out.map));
    }
    if has_errors(&out.diagnostics) {
        eprintln!(
            "{}: {} diagnostic{}",
            target.entry,
            out.diagnostics.len(),
            if out.diagnostics.len() == 1 { "" } else { "s" }
        );
        return ExitCode::FAILURE;
    }
    if let Err(n) = check_resolved_unit(&out, false, target.version) {
        eprintln!(
            "{}: {n} type diagnostic{}",
            target.entry,
            if n == 1 { "" } else { "s" }
        );
        return ExitCode::FAILURE;
    }
    print_bench_result(&target.entry, out.modules.len(), start.elapsed(), json);
    ExitCode::SUCCESS
}

pub(super) fn bench_self_product(
    product: Result<topaz_self_frontend::SelfCompilationProduct, ExitCode>,
    label: &str,
    started: std::time::Instant,
    json: bool,
    presentation: CheckPresentation,
) -> ExitCode {
    let product = match product {
        Ok(product) => product,
        Err(code) => return code,
    };
    if product.status() != "completed" {
        return check_self_compilation_product(product, label, false, false, presentation);
    }
    print_bench_result(
        label,
        product.typed().resolved.modules.len(),
        started.elapsed(),
        json,
    );
    ExitCode::SUCCESS
}

pub(super) fn bench_entry(
    entry: &str,
    root: Option<&str>,
    version: LangVersion,
    json: bool,
) -> ExitCode {
    if version == LangVersion::V5_1 {
        eprintln!(
            "topaz: `bench` needs v5.2+ (v5.16 is the default); `--language-version 5.1` has no module system"
        );
        return ExitCode::FAILURE;
    }
    let start = std::time::Instant::now();
    let entry = entry.replace('\\', "/");
    let (base, entry_rel, root_rel) = match split_absolute(&entry, root) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("topaz: {msg}");
            return ExitCode::FAILURE;
        }
    };
    let provider = PhysicalProvider::new(base);
    let out = resolve_with_version(&provider, &entry_rel, root_rel.as_deref(), version);
    for diag in &out.diagnostics {
        eprintln!("{}", render(diag, &out.map));
    }
    if has_errors(&out.diagnostics) {
        eprintln!(
            "{entry}: {} diagnostic{}",
            out.diagnostics.len(),
            if out.diagnostics.len() == 1 { "" } else { "s" }
        );
        return ExitCode::FAILURE;
    }
    if let Err(n) = check_resolved_unit(&out, false, version) {
        eprintln!(
            "{entry}: {n} type diagnostic{}",
            if n == 1 { "" } else { "s" }
        );
        return ExitCode::FAILURE;
    }
    print_bench_result(&entry, out.modules.len(), start.elapsed(), json);
    ExitCode::SUCCESS
}

pub(super) fn print_bench_result(
    entry: &str,
    modules: usize,
    elapsed: std::time::Duration,
    json: bool,
) {
    let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
    if json {
        let mut out = String::from("{\"status\":\"check-ok\",\"entry\":");
        push_json_string(&mut out, entry);
        let _ = write!(
            out,
            ",\"modules\":{modules},\"elapsedMs\":{elapsed_ms:.3}}}"
        );
        println!("{out}");
    } else {
        println!(
            "{entry}: bench check-ok ({} module{}) elapsed_ms={elapsed_ms:.3}",
            modules,
            if modules == 1 { "" } else { "s" },
        );
    }
}
