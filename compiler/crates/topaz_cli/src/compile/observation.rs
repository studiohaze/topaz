use crate::*;

pub(super) fn observation_schema(path: &str) -> &'static str {
    match path {
        "topaz-observation.json" => topaz_kernel::BUNDLE_SCHEMA,
        "request.json" => topaz_kernel::REQUEST_SCHEMA,
        "response.json" => topaz_kernel::RESPONSE_SCHEMA,
        "provenance.json" => topaz_kernel::PROVENANCE_SCHEMA,
        "source-set.jsonl" => topaz_kernel::SOURCE_SET_SCHEMA,
        "tokens.jsonl" => topaz_kernel::TOKENS_SCHEMA,
        "ast.jsonl" => topaz_kernel::AST_SCHEMA,
        "resolved.jsonl" => topaz_kernel::RESOLVED_SCHEMA,
        "typed.jsonl" => topaz_kernel::TYPED_SCHEMA,
        "lowered.jsonl" => topaz_kernel::LOWERED_SCHEMA,
        "rust-source.jsonl" => topaz_kernel::RUST_SOURCE_SCHEMA,
        "diagnostics.jsonl" => topaz_kernel::DIAGNOSTICS_SCHEMA,
        "stage1-product.json" => topaz_kernel::STAGE1_PRODUCT_SCHEMA,
        "stage2-product.json" => topaz_kernel::STAGE2_PRODUCT_SCHEMA,
        "stage2-fixed-point.json" => topaz_kernel::STAGE2_FIXED_POINT_SCHEMA,
        path if path.starts_with("sources/") && path.ends_with(".tpz") => "topaz.source/utf8",
        _ => "",
    }
}

pub(super) fn observation_relative_path(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path.strip_prefix(root).expect("walk remains under root");
    let relative = relative.to_str().ok_or_else(|| {
        format!(
            "observation member path `{}` cannot be represented as Unicode",
            relative.display()
        )
    })?;
    Ok(relative.replace('\\', "/"))
}

#[cfg(all(test, unix))]
#[test]
pub(super) fn observation_member_path_rejects_non_unicode_identity() {
    use std::os::unix::ffi::OsStringExt;

    let root = Path::new("bundle");
    let path = root.join(std::ffi::OsString::from_vec(b"member-\xff.json".to_vec()));
    let error = observation_relative_path(root, &path).expect_err("path must be rejected");
    assert!(
        error.contains("cannot be represented as Unicode"),
        "{error}"
    );
}

pub(super) fn collect_observation_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<topaz_kernel::ObservationFile>,
) -> Result<(), String> {
    let entries = fs::read_dir(current)
        .map_err(|error| format!("cannot read `{}`: {error}", current.display()))?;
    let mut entries = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot enumerate `{}`: {error}", current.display()))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect `{}`: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "observation bundle may not contain a symlink: `{}`",
                path.display()
            ));
        }
        if metadata.is_dir() {
            collect_observation_files(root, &path, files)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(format!(
                "observation bundle contains a non-file: `{}`",
                path.display()
            ));
        }
        let relative = observation_relative_path(root, &path)?;
        files.push(topaz_kernel::ObservationFile {
            schema: observation_schema(&relative).to_string(),
            path: relative,
            bytes: fs::read(&path)
                .map_err(|error| format!("cannot read `{}`: {error}", path.display()))?,
        });
    }
    Ok(())
}

pub(super) fn load_observation_bundle(
    root: &Path,
) -> Result<topaz_kernel::ObservationBundle, String> {
    if !root.is_dir() {
        return Err(format!(
            "observation path `{}` is not a directory",
            root.display()
        ));
    }
    let mut files = Vec::new();
    collect_observation_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let bundle = topaz_kernel::ObservationBundle { files };
    bundle.validate()?;
    Ok(bundle)
}

pub(super) fn unique_observation_sibling(path: &Path, role: &str) -> Result<PathBuf, String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("observation");
    for ordinal in 0..100_u32 {
        let candidate = parent.join(format!(
            ".topaz-{name}-{role}-{}-{ordinal}",
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("cannot allocate an observation transaction sibling".to_string())
}

pub(super) fn collect_observation_paths(
    root: &Path,
    current: &Path,
    paths: &mut BTreeSet<String>,
) -> Result<(), String> {
    let entries = fs::read_dir(current)
        .map_err(|error| format!("cannot read `{}`: {error}", current.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("cannot enumerate `{}`: {error}", current.display()))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect `{}`: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "observation bundle may not contain a symlink: `{}`",
                path.display()
            ));
        }
        if metadata.is_dir() {
            collect_observation_paths(root, &path, paths)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(format!(
                "observation bundle contains a non-file: `{}`",
                path.display()
            ));
        }
        let relative = observation_relative_path(root, &path)?;
        if !paths.insert(relative.clone()) {
            return Err(format!("observation bundle repeats `{relative}`"));
        }
    }
    Ok(())
}

pub(super) fn verify_written_observation_bundle(
    root: &Path,
    bundle: &topaz_kernel::ObservationBundle,
) -> Result<(), String> {
    let expected = bundle
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();
    collect_observation_paths(root, root, &mut actual)?;
    if actual != expected {
        return Err("written observation file set differs from its validated bundle".to_string());
    }
    for file in &bundle.files {
        let path = root.join(&file.path);
        let written = fs::read(&path)
            .map_err(|error| format!("cannot read `{}`: {error}", path.display()))?;
        if written != file.bytes {
            return Err(format!(
                "written observation member `{}` differs from its validated bytes",
                file.path
            ));
        }
    }
    Ok(())
}

pub(super) fn write_observation_bundle(
    path: &Path,
    bundle: &topaz_kernel::ObservationBundle,
) -> Result<(), String> {
    bundle.validate()?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create `{}`: {error}", parent.display()))?;
    if path.exists() {
        load_observation_bundle(path).map_err(|error| {
            format!(
                "cannot replace unmanaged observation directory `{}`: {error}",
                path.display()
            )
        })?;
    }
    let staging = unique_observation_sibling(path, "staging")?;
    fs::create_dir(&staging)
        .map_err(|error| format!("cannot create `{}`: {error}", staging.display()))?;
    let write_result = (|| {
        for file in &bundle.files {
            let target = staging.join(&file.path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("cannot create `{}`: {error}", parent.display()))?;
            }
            fs::write(&target, &file.bytes)
                .map_err(|error| format!("cannot write `{}`: {error}", target.display()))?;
        }
        verify_written_observation_bundle(&staging, bundle)?;
        Ok::<_, String>(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if !path.exists() {
        return fs::rename(&staging, path)
            .map_err(|error| format!("cannot install `{}`: {error}", path.display()));
    }
    let backup = unique_observation_sibling(path, "backup")?;
    fs::rename(path, &backup)
        .map_err(|error| format!("cannot stage previous observation: {error}"))?;
    if let Err(error) = fs::rename(&staging, path) {
        let _ = fs::rename(&backup, path);
        return Err(format!("cannot install new observation: {error}"));
    }
    fs::remove_dir_all(&backup)
        .map_err(|error| format!("cannot remove replaced managed observation: {error}"))
}

pub(super) fn canonical_future_path(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("cannot determine the current directory: {error}"))?
            .join(path)
    };
    let mut existing = absolute.as_path();
    let mut suffix = Vec::new();
    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            format!(
                "cannot resolve observation destination `{}`",
                path.display()
            )
        })?;
        suffix.push(name.to_os_string());
        existing = existing.parent().ok_or_else(|| {
            format!(
                "cannot resolve observation destination `{}`",
                path.display()
            )
        })?;
    }
    let mut resolved = fs::canonicalize(existing)
        .map_err(|error| format!("cannot resolve `{}`: {error}", existing.display()))?;
    for component in suffix.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

pub(super) fn require_observation_outside_source(
    source_root: &Path,
    out_dir: &str,
) -> Result<(), String> {
    let source_root = fs::canonicalize(source_root).map_err(|error| {
        format!(
            "cannot resolve observed source root `{}`: {error}",
            source_root.display()
        )
    })?;
    let destination = canonical_future_path(Path::new(out_dir))?;
    if destination.starts_with(&source_root) {
        return Err(format!(
            "compiler observation output `{out_dir}` must be outside the observed source root `{}`",
            source_root.display()
        ));
    }
    Ok(())
}

pub(super) struct CompilerObserveRequest<'request> {
    pub(super) entry: Option<&'request str>,
    pub(super) root: Option<&'request str>,
    pub(super) version_arg: Option<LangVersion>,
    pub(super) version: LangVersion,
    pub(super) locked: bool,
    pub(super) out_dir: Option<&'request str>,
    pub(super) terminal: topaz_kernel::TerminalPhase,
    pub(super) compiler_selection: CompilerSelection,
}

pub(super) fn compiler_observe(request: CompilerObserveRequest<'_>) -> ExitCode {
    let CompilerObserveRequest {
        entry,
        root,
        version_arg,
        version,
        locked,
        out_dir,
        terminal,
        compiler_selection,
    } = request;
    if terminal == topaz_kernel::TerminalPhase::Ast {
        eprintln!("topaz: `compiler observe` supports only `--terminal typed` or `rust-source`");
        return ExitCode::FAILURE;
    }
    if !version.uses_self_hosted_product_default() {
        eprintln!(
            "topaz: `compiler observe` requires a language profile admitted by the self-hosted compiler"
        );
        return ExitCode::FAILURE;
    }
    let Some(out_dir) = out_dir else {
        eprintln!("topaz: `compiler observe` requires --out-dir");
        return ExitCode::FAILURE;
    };
    if compiler_selection == CompilerSelection::SelfHosted {
        let product = match entry {
            Some(entry) => {
                if locked {
                    eprintln!("topaz: `--locked` applies only to package observation");
                    return ExitCode::FAILURE;
                }
                let normalized = entry.replace('\\', "/");
                let (base, entry_relative, root_relative) = match split_absolute(&normalized, root)
                {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("topaz: {error}");
                        return ExitCode::FAILURE;
                    }
                };
                let logical_root = root_relative.clone().unwrap_or_else(|| {
                    Path::new(&entry_relative)
                        .parent()
                        .map_or_else(String::new, |parent| {
                            parent.to_string_lossy().replace('\\', "/")
                        })
                });
                if let Err(error) = require_observation_outside_source(
                    &Path::new(&base).join(&logical_root),
                    out_dir,
                ) {
                    eprintln!("topaz: {error}");
                    return ExitCode::FAILURE;
                }
                let request = topaz_kernel::KernelRequest::checked(
                    &entry_relative,
                    root_relative.as_deref(),
                    version,
                    topaz_kernel::PackageFacts::standalone(),
                );
                match compile_self_product(
                    &PhysicalFactHost::new(base),
                    request,
                    None,
                    "rerun `topaz compiler observe --compiler rust`",
                ) {
                    Ok(product) => product,
                    Err(code) => return code,
                }
            }
            None => {
                let target = match package_target(root, version_arg, locked) {
                    Ok(target) => target,
                    Err(code) => return code,
                };
                if let Err(error) = require_observation_outside_source(&target.root, out_dir) {
                    eprintln!("topaz: {error}");
                    return ExitCode::FAILURE;
                }
                match compile_self_package_product(&target, None, "compiler observe") {
                    Ok(product) => product,
                    Err(code) => return code,
                }
            }
        };
        let bundle =
            match topaz_self_frontend::build_self_compilation_observation(&product, terminal) {
                Ok(bundle) => bundle,
                Err(error) => {
                    eprintln!("topaz: cannot build self compiler observation: {error}");
                    return ExitCode::FAILURE;
                }
            };
        if let Err(error) = write_observation_bundle(Path::new(out_dir), &bundle) {
            eprintln!("topaz: cannot write self compiler observation: {error}");
            return ExitCode::FAILURE;
        }
        println!("{out_dir}: compiler-observation-ok");
        return ExitCode::SUCCESS;
    }
    let execution = match entry {
        Some(entry) => {
            if locked {
                eprintln!("topaz: `--locked` applies only to package observation");
                return ExitCode::FAILURE;
            }
            let entry = entry.replace('\\', "/");
            let (base, entry_rel, root_rel) = match split_absolute(&entry, root) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("topaz: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let logical_root = root_rel.clone().unwrap_or_else(|| {
                Path::new(&entry_rel)
                    .parent()
                    .map_or_else(String::new, |parent| {
                        parent.to_string_lossy().replace('\\', "/")
                    })
            });
            if let Err(error) =
                require_observation_outside_source(&Path::new(&base).join(&logical_root), out_dir)
            {
                eprintln!("topaz: {error}");
                return ExitCode::FAILURE;
            }
            let request = topaz_kernel::KernelRequest::checked(
                &entry_rel,
                root_rel.as_deref(),
                version,
                topaz_kernel::PackageFacts::standalone(),
            )
            .with_terminal_phase(terminal);
            topaz_kernel::drive_checked(&PhysicalFactHost::new(base), request)
        }
        None => {
            let target = match package_target(root, version_arg, locked) {
                Ok(target) => target,
                Err(code) => return code,
            };
            if let Err(error) = require_observation_outside_source(&target.root, out_dir) {
                eprintln!("topaz: {error}");
                return ExitCode::FAILURE;
            }
            let request = topaz_kernel::KernelRequest::checked(
                &target.entry,
                Some(""),
                target.version,
                package_kernel_facts(&target),
            )
            .with_terminal_phase(terminal);
            topaz_kernel::drive_checked(&PackageFactHost::new(&target), request)
        }
    };
    let bundle = match topaz_kernel::build_observation(&execution) {
        Ok(bundle) => bundle,
        Err(error) => {
            eprintln!("topaz: cannot build compiler observation: {error}");
            return ExitCode::FAILURE;
        }
    };
    drop(execution);
    if let Err(error) = write_observation_bundle(Path::new(out_dir), &bundle) {
        eprintln!("topaz: cannot write compiler observation: {error}");
        return ExitCode::FAILURE;
    }
    println!("{out_dir}: compiler-observation-ok");
    ExitCode::SUCCESS
}

pub(super) fn compiler_producer_preview_failure(stage: u8, error: &str) -> ExitCode {
    eprintln!("topaz: Stage {stage} Compiler Preview stopped: {error}");
    eprintln!(
        "topaz: recovery: run `topaz compiler observe [entry] --terminal rust-source --out-dir <directory>` as a new explicit command"
    );
    ExitCode::FAILURE
}

pub(super) fn compiler_status(json: bool) -> ExitCode {
    let identity = topaz_self_frontend::installed_stage2_identity();
    let status_identity = identity.as_ref().map_err(String::as_str);
    let language_mode = format!("topaz-{}", LangVersion::CURRENT.as_str());
    if json {
        print!(
            "{}",
            compiler_support::status_json(
                env!("CARGO_PKG_VERSION"),
                &language_mode,
                status_identity
            )
        );
    } else {
        print!(
            "{}",
            compiler_support::status_human(
                env!("CARGO_PKG_VERSION"),
                &language_mode,
                status_identity,
            )
        );
    }
    ExitCode::SUCCESS
}

pub(super) fn run_compiler_producer_preview(
    source: &dyn topaz_kernel::HostFactSource,
    request: topaz_kernel::KernelRequest,
    out_dir: &str,
    producer: PreviewProducer,
    self_source: bool,
) -> ExitCode {
    let stage = match producer {
        PreviewProducer::Stage1 => 1,
        PreviewProducer::Stage2 => 2,
    };
    let generated = match producer {
        PreviewProducer::Stage1 => {
            topaz_self_frontend::preview_linked_stage1_generated(source, request.clone())
        }
        PreviewProducer::Stage2 => {
            topaz_self_frontend::preview_linked_stage2_generated(source, request.clone())
        }
    };
    let mut generated = match generated {
        Ok(result) => result,
        Err(error) => return compiler_producer_preview_failure(stage, &error),
    };
    if producer == PreviewProducer::Stage2
        && self_source
        && let Err(error) = topaz_self_frontend::seal_compiler_program_target_facts(&mut generated)
    {
        return compiler_producer_preview_failure(stage, &error);
    }
    let lowered = match topaz_self_frontend::decode_stage1_lowering_from_generated(&generated) {
        Ok(result) => result,
        Err(error) => return compiler_producer_preview_failure(stage, &error),
    };
    if lowered.status != "completed" || !lowered.unsupported.is_empty() {
        return compiler_producer_preview_failure(
            stage,
            &format!("unsupported target shape: {:?}", lowered.unsupported),
        );
    }
    let typed = match topaz_self_frontend::decode_stage1_typed_from_generated(&generated) {
        Ok(result) => result,
        Err(error) => return compiler_producer_preview_failure(stage, &error),
    };
    let typed_bundle =
        match topaz_kernel::build_typed_preview_observation(typed.observation_input()) {
            Ok(bundle) => bundle,
            Err(error) => return compiler_producer_preview_failure(stage, &error),
        };
    let Some(source_set) = typed_bundle
        .files
        .iter()
        .find(|file| file.path == "source-set.jsonl")
    else {
        return compiler_producer_preview_failure(stage, "typed base omitted source-set.jsonl");
    };
    let target_source_set_id = {
        let digest = topaz_value::value::sha256(&source_set.bytes);
        let mut value = String::from("sha256:");
        topaz_value::bytes_to_hex_into(&mut value, &digest);
        value
    };
    let lowered_jsonl = match topaz_self_frontend::encode_stage1_lowered_projection(&lowered) {
        Ok(bytes) => bytes,
        Err(error) => return compiler_producer_preview_failure(stage, &error),
    };
    let product = match producer {
        PreviewProducer::Stage1 => {
            topaz_self_frontend::encode_stage1_product_manifest(&generated, &target_source_set_id)
        }
        PreviewProducer::Stage2 => topaz_self_frontend::encode_stage2_product_manifest(
            &generated,
            &target_source_set_id,
            self_source,
        ),
    };
    let product = match product {
        Ok(bytes) => bytes,
        Err(error) => return compiler_producer_preview_failure(stage, &error),
    };
    let fixed_point = if producer == PreviewProducer::Stage2
        && self_source
        && std::env::var_os("TOPAZ_STAGE2_FIXED_POINT_RECORD").is_some()
    {
        match topaz_self_frontend::encode_stage2_fixed_point_record(&generated) {
            Ok(bytes) => Some(bytes),
            Err(error) => return compiler_producer_preview_failure(stage, &error),
        }
    } else {
        None
    };
    let bundle = match topaz_kernel::complete_compiler_preview_observation(
        typed_bundle,
        &generated.request,
        topaz_kernel::CompilerPreviewCompletion {
            lowered_jsonl,
            generated_rust: &generated.generated_rust,
            product,
            runtime_template_identity: topaz_self_frontend::FIXED_POINT_RUNTIME_TEMPLATE,
            runtime_template_sha256: topaz_self_frontend::FIXED_POINT_RUNTIME_TEMPLATE_SHA256,
            producer_stage: stage,
            fixed_point,
        },
    ) {
        Ok(bundle) => bundle,
        Err(error) => return compiler_producer_preview_failure(stage, &error),
    };
    if let Err(error) = write_observation_bundle(Path::new(out_dir), &bundle) {
        return compiler_producer_preview_failure(
            stage,
            &format!("cannot write compiler observation: {error}"),
        );
    }
    println!("{out_dir}: compiler-stage{stage}-preview-ok");
    ExitCode::SUCCESS
}

pub(super) struct CompilerPreviewRequest<'request> {
    pub(super) entry: Option<&'request str>,
    pub(super) root: Option<&'request str>,
    pub(super) version_arg: Option<LangVersion>,
    pub(super) version: LangVersion,
    pub(super) locked: bool,
    pub(super) out_dir: Option<&'request str>,
    pub(super) terminal: topaz_kernel::TerminalPhase,
    pub(super) preview_producer: Option<PreviewProducer>,
    pub(super) self_source: bool,
}

pub(super) fn compiler_preview(request: CompilerPreviewRequest<'_>) -> ExitCode {
    let CompilerPreviewRequest {
        entry,
        root,
        version_arg,
        version,
        locked,
        out_dir,
        terminal,
        preview_producer,
        self_source,
    } = request;
    if !version.uses_self_hosted_product_default() {
        eprintln!(
            "topaz: `compiler preview` requires a language profile admitted by the self-hosted compiler"
        );
        return ExitCode::FAILURE;
    }
    let Some(out_dir) = out_dir else {
        eprintln!("topaz: `compiler preview` requires --out-dir");
        return ExitCode::FAILURE;
    };
    if self_source && entry.is_some() {
        eprintln!("topaz: `--self-source` does not accept an entry path");
        return ExitCode::FAILURE;
    }
    if self_source {
        if locked {
            eprintln!("topaz: `--locked` does not apply to `--self-source`");
            return ExitCode::FAILURE;
        }
        let mut request = topaz_kernel::KernelRequest::checked(
            "src/main.tpz",
            Some(""),
            version,
            topaz_kernel::PackageFacts::standalone(),
        )
        .with_terminal_phase(topaz_kernel::TerminalPhase::RustSource);
        if let Err(error) = topaz_self_frontend::supply_embedded_compiler_source_facts(&mut request)
        {
            return compiler_producer_preview_failure(
                2,
                &format!("cannot seed embedded compiler source: {error:?}"),
            );
        }
        return run_compiler_producer_preview(
            &topaz_self_frontend::EmbeddedCompilerSourceHost,
            request,
            out_dir,
            PreviewProducer::Stage2,
            true,
        );
    }
    if let Some(preview_producer) = preview_producer {
        match entry {
            Some(entry) => {
                if locked {
                    eprintln!("topaz: `--locked` applies only to package preview");
                    return ExitCode::FAILURE;
                }
                let entry = entry.replace('\\', "/");
                let (base, entry_rel, root_rel) = match split_absolute(&entry, root) {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("topaz: {error}");
                        return ExitCode::FAILURE;
                    }
                };
                let logical_root = root_rel.clone().unwrap_or_else(|| {
                    Path::new(&entry_rel)
                        .parent()
                        .map_or_else(String::new, |parent| {
                            parent.to_string_lossy().replace('\\', "/")
                        })
                });
                if let Err(error) = require_observation_outside_source(
                    &Path::new(&base).join(&logical_root),
                    out_dir,
                ) {
                    eprintln!("topaz: {error}");
                    return ExitCode::FAILURE;
                }
                let request = topaz_kernel::KernelRequest::checked(
                    &entry_rel,
                    root_rel.as_deref(),
                    version,
                    topaz_kernel::PackageFacts::standalone(),
                )
                .with_terminal_phase(topaz_kernel::TerminalPhase::RustSource);
                return run_compiler_producer_preview(
                    &PhysicalFactHost::new(base),
                    request,
                    out_dir,
                    preview_producer,
                    false,
                );
            }
            None => {
                let target = match package_target(root, version_arg, locked) {
                    Ok(target) => target,
                    Err(code) => return code,
                };
                if !target.version.uses_self_hosted_product_default() {
                    eprintln!(
                        "topaz: `compiler preview` requires a language profile admitted by the self-hosted compiler"
                    );
                    return ExitCode::FAILURE;
                }
                if let Err(error) = require_observation_outside_source(&target.root, out_dir) {
                    eprintln!("topaz: {error}");
                    return ExitCode::FAILURE;
                }
                let request = topaz_kernel::KernelRequest::checked(
                    &target.entry,
                    Some(""),
                    target.version,
                    package_kernel_facts(&target),
                )
                .with_terminal_phase(topaz_kernel::TerminalPhase::RustSource);
                return run_compiler_producer_preview(
                    &PackageFactHost::new(&target),
                    request,
                    out_dir,
                    preview_producer,
                    false,
                );
            }
        }
    }
    if terminal == topaz_kernel::TerminalPhase::Ast {
        let Some(entry) = entry else {
            eprintln!("topaz: AST-terminal `compiler preview` requires an entry file");
            return ExitCode::FAILURE;
        };
        if locked {
            eprintln!("topaz: `--locked` applies only to package preview");
            return ExitCode::FAILURE;
        }
        let entry = entry.replace('\\', "/");
        let (base, entry_rel, root_rel) = match split_absolute(&entry, root) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("topaz: {error}");
                return ExitCode::FAILURE;
            }
        };
        let logical_root = root_rel.clone().unwrap_or_else(|| {
            Path::new(&entry_rel)
                .parent()
                .map_or_else(String::new, |parent| {
                    parent.to_string_lossy().replace('\\', "/")
                })
        });
        if let Err(error) =
            require_observation_outside_source(&Path::new(&base).join(&logical_root), out_dir)
        {
            eprintln!("topaz: {error}");
            return ExitCode::FAILURE;
        }
        let source = match fs::read_to_string(Path::new(&base).join(&entry_rel)) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("topaz: cannot read `{entry_rel}`: {error}");
                return ExitCode::FAILURE;
            }
        };
        let module = Path::new(&entry_rel)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(&entry_rel);
        let preview = match topaz_self_frontend::preview_source(&entry_rel, &source) {
            Ok(preview) => preview,
            Err(error) => {
                eprintln!("topaz: front-end preview stopped: {error}");
                return ExitCode::FAILURE;
            }
        };
        let bundle = match topaz_kernel::build_ast_preview_observation(
            &entry_rel,
            module,
            &source,
            &preview.raw,
            &preview.layout,
            &preview.ast,
            &preview.diagnostics,
        ) {
            Ok(bundle) => bundle,
            Err(error) => {
                eprintln!("topaz: cannot build front-end preview observation: {error}");
                return ExitCode::FAILURE;
            }
        };
        if let Err(error) = write_observation_bundle(Path::new(out_dir), &bundle) {
            eprintln!("topaz: cannot write front-end preview observation: {error}");
            return ExitCode::FAILURE;
        }
        println!("{out_dir}: compiler-preview-ok");
        return ExitCode::SUCCESS;
    }
    if terminal != topaz_kernel::TerminalPhase::Typed {
        eprintln!("topaz: `compiler preview` supports only `--terminal ast` or `typed`");
        return ExitCode::FAILURE;
    }
    let preview = match entry {
        Some(entry) => {
            if locked {
                eprintln!("topaz: `--locked` applies only to package preview");
                return ExitCode::FAILURE;
            }
            let entry = entry.replace('\\', "/");
            let (base, entry_rel, root_rel) = match split_absolute(&entry, root) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("topaz: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let logical_root = root_rel.clone().unwrap_or_else(|| {
                Path::new(&entry_rel)
                    .parent()
                    .map_or_else(String::new, |parent| {
                        parent.to_string_lossy().replace('\\', "/")
                    })
            });
            if let Err(error) =
                require_observation_outside_source(&Path::new(&base).join(&logical_root), out_dir)
            {
                eprintln!("topaz: {error}");
                return ExitCode::FAILURE;
            }
            let request = topaz_kernel::KernelRequest::checked(
                &entry_rel,
                root_rel.as_deref(),
                version,
                topaz_kernel::PackageFacts::standalone(),
            )
            .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
            match topaz_self_frontend::preview_typed(&PhysicalFactHost::new(base), request) {
                Ok(preview) => preview,
                Err(error) => {
                    eprintln!("topaz: front-end preview stopped: {error}");
                    return ExitCode::FAILURE;
                }
            }
        }
        None => {
            let target = match package_target(root, version_arg, locked) {
                Ok(target) => target,
                Err(code) => return code,
            };
            if !target.version.uses_self_hosted_product_default() {
                eprintln!(
                    "topaz: `compiler preview` requires a language profile admitted by the self-hosted compiler"
                );
                return ExitCode::FAILURE;
            }
            if let Err(error) = require_observation_outside_source(&target.root, out_dir) {
                eprintln!("topaz: {error}");
                return ExitCode::FAILURE;
            }
            let request = topaz_kernel::KernelRequest::checked(
                &target.entry,
                Some(""),
                target.version,
                package_kernel_facts(&target),
            )
            .with_terminal_phase(topaz_kernel::TerminalPhase::Typed);
            match topaz_self_frontend::preview_typed(&PackageFactHost::new(&target), request) {
                Ok(preview) => preview,
                Err(error) => {
                    eprintln!("topaz: front-end preview stopped: {error}");
                    return ExitCode::FAILURE;
                }
            }
        }
    };
    let bundle = match topaz_kernel::build_typed_preview_observation(preview.observation_input()) {
        Ok(bundle) => bundle,
        Err(error) => {
            eprintln!("topaz: cannot build front-end preview observation: {error}");
            return ExitCode::FAILURE;
        }
    };
    let fact_rounds = preview.resolved.rounds;
    drop(preview);
    if let Err(error) = write_observation_bundle(Path::new(out_dir), &bundle) {
        eprintln!("topaz: cannot write front-end preview observation: {error}");
        return ExitCode::FAILURE;
    }
    if std::env::var_os("TOPAZ_SELF_FRONTEND_METRICS").is_some() {
        println!("topaz-self-frontend-fact-rounds: {fact_rounds}");
    }
    println!("{out_dir}: compiler-preview-ok");
    ExitCode::SUCCESS
}

pub(super) fn compiler_validate_observation(path: &str) -> ExitCode {
    match load_observation_bundle(Path::new(path)) {
        Ok(_) => {
            println!("{path}: compiler-observation-valid");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("topaz: invalid compiler observation `{path}`: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(super) fn compiler_compare(
    left_path: &str,
    right_path: &str,
    layer: topaz_kernel::ComparisonLayer,
) -> ExitCode {
    let record = if layer == topaz_kernel::ComparisonLayer::NativeBinary {
        let left = match fs::read(left_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("topaz: cannot read native binary `{left_path}`: {error}");
                return ExitCode::FAILURE;
            }
        };
        let right = match fs::read(right_path) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("topaz: cannot read native binary `{right_path}`: {error}");
                return ExitCode::FAILURE;
            }
        };
        topaz_kernel::compare_native_binaries(&left, &right)
    } else {
        let left = match load_observation_bundle(Path::new(left_path)) {
            Ok(bundle) => bundle,
            Err(error) => {
                eprintln!("topaz: invalid left compiler observation `{left_path}`: {error}");
                return ExitCode::FAILURE;
            }
        };
        let right = match load_observation_bundle(Path::new(right_path)) {
            Ok(bundle) => bundle,
            Err(error) => {
                eprintln!("topaz: invalid right compiler observation `{right_path}`: {error}");
                return ExitCode::FAILURE;
            }
        };
        match topaz_kernel::compare_observations(&left, &right, layer) {
            Ok(record) => record,
            Err(error) => {
                eprintln!("topaz: cannot compare compiler observations: {error}");
                return ExitCode::FAILURE;
            }
        }
    };
    if let Err(error) = std::io::stdout().write_all(&record.bytes) {
        eprintln!("topaz: cannot write compiler comparison: {error}");
        return ExitCode::FAILURE;
    }
    if record.equal {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
