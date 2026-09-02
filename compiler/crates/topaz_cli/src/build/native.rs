use crate::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn build_native_artifact(
    dir: &Path,
    entry: &str,
    version: LangVersion,
    rust: &str,
    compiler: artifact::CompilerProvenance,
    harness: HostHarness<'_>,
    explicit_main: bool,
    release: bool,
    run: bool,
    program_args: &[String],
) -> ExitCode {
    if let Err(code) = admit_cli_program_args(explicit_main, program_args) {
        return code;
    }
    let lispex_application = harness.lispex_application();
    let destination = match artifact::Destination::open(dir, artifact::Target::Native) {
        Ok(destination) => destination,
        Err(e) => {
            eprintln!("topaz: cannot use output directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let workspace = match storage::Workspace::create() {
        Ok(workspace) => workspace,
        Err(e) => {
            eprintln!("topaz: cannot create build workspace: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = scaffold_crate(&workspace.source, rust, harness) {
        eprintln!("topaz: could not scaffold the temporary native crate: {e}");
        return ExitCode::FAILURE;
    }
    let env = match prepare_workspace_build_env(workspace, dir) {
        Ok(env) => env,
        Err(code) => return code,
    };
    if let Err(code) = generate_lockfile(&env) {
        return code;
    }
    let profile = if release { "release" } else { "debug" };
    eprintln!("topaz: compiling `{entry}` (offline, locked, isolated) …");
    let mut cmd = env.cargo();
    cmd.args(["build", "--offline", "--locked", "--manifest-path"])
        .arg(&env.manifest);
    if release {
        cmd.arg("--release");
    }
    if let Err(code) = run_cargo_logged(&env, "build", cmd) {
        return code;
    }
    let built = env
        .target
        .join(profile)
        .join(format!("program{}", std::env::consts::EXE_SUFFIX));
    let bytes = match fs::read(&built) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("topaz: native build succeeded but its final binary is missing: {e}");
            return ExitCode::FAILURE;
        }
    };
    let relative = format!("target/{profile}/program{}", std::env::consts::EXE_SUFFIX);
    let mut files = vec![artifact::File::binary(&relative, bytes, true)];
    if let Some(application) = lispex_application {
        files.extend(
            application
                .payload
                .files
                .iter()
                .map(|file| artifact::File::binary(&file.path, file.bytes.clone(), false)),
        );
        files.push(artifact::File::text(
            "lispex/RUNTIME-THIRD-PARTY-NOTICES.txt",
            LISPEX_APPLICATION_THIRD_PARTY_NOTICES,
        ));
    }
    let plan = artifact::Plan {
        target: artifact::Target::Native,
        language_version: version,
        entry: logical_entry(entry),
        runtime_requirements: vec![format!(
            "{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )],
        invocation: format!("./{relative}"),
        compiler: Some(compiler),
        files,
    };
    if let Err(e) = destination.commit(plan) {
        eprintln!(
            "topaz: could not install native artifact in `{}`: {e}",
            dir.display()
        );
        return ExitCode::FAILURE;
    }
    let installed = dir.join(&relative);
    eprintln!("topaz: built `{}`", installed.display());
    if !run {
        return ExitCode::SUCCESS;
    }
    match std::process::Command::new(&installed)
        .args(program_args)
        .status()
    {
        Ok(status) => ExitCode::from(u8::try_from(status.code().unwrap_or(1)).unwrap_or(1)),
        Err(e) => {
            eprintln!(
                "topaz: built `{}` but could not run it: {e}",
                installed.display()
            );
            ExitCode::FAILURE
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_http_service_artifact(
    dir: &Path,
    entry: &str,
    version: LangVersion,
    rust: &str,
    compiler: artifact::CompilerProvenance,
    harness: HostHarness<'_>,
    config: &topaz_package::ServiceConfig,
    release: bool,
) -> ExitCode {
    let destination = match artifact::Destination::open(dir, artifact::Target::HttpService) {
        Ok(destination) => destination,
        Err(e) => {
            eprintln!("topaz: cannot use output directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let workspace = match storage::Workspace::create() {
        Ok(workspace) => workspace,
        Err(e) => {
            eprintln!("topaz: cannot create build workspace: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = scaffold_service_crate(&workspace.source, rust, harness, config) {
        eprintln!("topaz: could not scaffold the temporary http-service crate: {e}");
        return ExitCode::FAILURE;
    }
    let env = match prepare_workspace_build_env(workspace, dir) {
        Ok(env) => env,
        Err(code) => return code,
    };
    if let Err(code) = generate_lockfile(&env) {
        return code;
    }
    let profile = if release { "release" } else { "debug" };
    eprintln!("topaz: compiling `{entry}` as a bounded HTTP service (offline, locked, isolated) …");
    let mut cmd = env.cargo();
    cmd.args(["build", "--offline", "--locked", "--manifest-path"])
        .arg(&env.manifest);
    if release {
        cmd.arg("--release");
    }
    if let Err(code) = run_cargo_logged(&env, "build", cmd) {
        return code;
    }
    let built = env
        .target
        .join(profile)
        .join(format!("program{}", std::env::consts::EXE_SUFFIX));
    let bytes = match fs::read(&built) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("topaz: http-service build succeeded but its final binary is missing: {e}");
            return ExitCode::FAILURE;
        }
    };
    let relative = format!("target/{profile}/program{}", std::env::consts::EXE_SUFFIX);
    let plan = artifact::Plan {
        target: artifact::Target::HttpService,
        language_version: version,
        entry: logical_entry(entry),
        runtime_requirements: vec![format!(
            "{}-{}; loopback by default; bounded HTTP/1.1",
            std::env::consts::OS,
            std::env::consts::ARCH
        )],
        invocation: format!("./{relative} [--print-config] [--bind <ip>] [--port <port>]"),
        compiler: Some(compiler),
        files: vec![
            artifact::File::binary(&relative, bytes, true),
            artifact::File::text(
                "topaz-service-config.json",
                service_artifact_config_json(config),
            ),
            artifact::File::text("THIRD-PARTY-NOTICES.txt", SERVICE_THIRD_PARTY_NOTICES),
        ],
    };
    if let Err(e) = destination.commit(plan) {
        eprintln!(
            "topaz: could not install http-service artifact in `{}`: {e}",
            dir.display()
        );
        return ExitCode::FAILURE;
    }
    let installed = dir.join(&relative);
    eprintln!(
        "topaz: built bounded HTTP service `{}`",
        installed.display()
    );
    ExitCode::SUCCESS
}

pub(super) fn service_artifact_config_json(config: &topaz_package::ServiceConfig) -> String {
    let log_format = match config.log_format {
        topaz_package::ServiceLogFormat::Text => "text",
        topaz_package::ServiceLogFormat::Json => "json",
        topaz_package::ServiceLogFormat::Off => "off",
    };
    format!(
        concat!(
            "{{\n",
            "  \"schema\": \"topaz.httpServiceConfig.v1\",\n",
            "  \"source\": \"embedded-defaults\",\n",
            "  \"transport\": \"http1\",\n",
            "  \"runtimeOverrides\": \"command-line\",\n",
            "  \"nonLoopbackBind\": \"explicit-only\",\n",
            "  \"values\": {{\n",
            "    \"bind\": \"{}\",\n",
            "    \"port\": {},\n",
            "    \"workers\": {},\n",
            "    \"maxConnections\": {},\n",
            "    \"queueCapacity\": {},\n",
            "    \"maxTargetBytes\": {},\n",
            "    \"maxHeaderBytes\": {},\n",
            "    \"maxHeaders\": {},\n",
            "    \"maxBodyBytes\": {},\n",
            "    \"headerTimeoutMs\": {},\n",
            "    \"bodyTimeoutMs\": {},\n",
            "    \"handlerTimeoutMs\": {},\n",
            "    \"shutdownGraceMs\": {},\n",
            "    \"logFormat\": \"{}\"\n",
            "  }}\n",
            "}}\n"
        ),
        config.bind,
        config.port,
        config.workers,
        config.max_connections,
        config.queue_capacity,
        config.max_target_bytes,
        config.max_header_bytes,
        config.max_headers,
        config.max_body_bytes,
        config.header_timeout_ms,
        config.body_timeout_ms,
        config.handler_timeout_ms,
        config.shutdown_grace_ms,
        log_format,
    )
}

pub(super) fn rust_string_vec_literal(values: &[String]) -> String {
    if values.is_empty() {
        return "Vec::<String>::new()".to_string();
    }
    let items = values
        .iter()
        .map(|value| format!("String::from({})", rust_string_literal(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("vec![{items}]")
}

pub(super) fn rust_extern_sandbox_policy_vec_literal(
    policies: &[topaz_value::ExternSandboxPolicy],
) -> String {
    if policies.is_empty() {
        return "Vec::<topaz_rt::ExternSandboxPolicy>::new()".to_string();
    }
    let items = policies
        .iter()
        .map(|policy| {
            let module = rust_string_literal(&policy.module);
            let kind = match policy.kind {
                topaz_value::ExternSandboxKind::Replay => "topaz_rt::ExternSandboxKind::Replay",
                topaz_value::ExternSandboxKind::Wasm => "topaz_rt::ExternSandboxKind::Wasm",
            };
            let artifact_path = match &policy.artifact_path {
                Some(path) => format!("Some(String::from({}))", rust_string_literal(path)),
                None => "None".to_string(),
            };
            let fuel = rust_optional_u64_literal(policy.fuel);
            let memory_bytes = rust_optional_u64_literal(policy.memory_bytes);
            format!(
                "topaz_rt::ExternSandboxPolicy::new({module}, {kind}, {artifact_path}, {fuel}, {memory_bytes})"
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("vec![{items}]")
}

pub(super) fn rust_optional_u64_literal(value: Option<u64>) -> String {
    match value {
        Some(value) => format!("Some({value})"),
        None => "None".to_string(),
    }
}

pub(super) fn rust_string_literal(value: &str) -> String {
    format!("{value:?}")
}

// The CDR-006 §7 vendored runtime closure, embedded in this binary at compile
// time (see build.rs): `VENDOR_FILES`, `VENDOR_WORKSPACE_MANIFEST`,
// `VENDOR_TOOLCHAIN`.
include!(concat!(env!("OUT_DIR"), "/vendor.rs"));

/// Write the embedded runtime closure into `<out_dir>/vendor/` (CDR-006 §7): the
/// byte-identical closure crates under `vendor/crates/<crate>/…` plus the
/// synthesized `vendor/Cargo.toml` workspace root they inherit from. The emitted
/// program then depends on `vendor/crates/…` by relative path, so the tree is
/// self-contained — a binary-only user needs no source checkout or registry.
pub(super) fn expand_vendor(
    out_dir: &Path,
    service: bool,
    lispex_application: bool,
    full_lispex_application: bool,
) -> std::io::Result<()> {
    let vendor = out_dir.join("vendor");
    // The vendor tree is compiler-owned; wipe it first so a reused `--out-dir`
    // cannot leave a stale file (e.g. an old `build.rs`) that Cargo would still
    // discover and run — that would defeat the embedded closure's audit. Fail
    // closed: tolerate only "not there yet"; any other removal error propagates
    // (a half-deleted vendor tree must not be silently re-expanded over).
    match fs::remove_dir_all(&vendor) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    for (rel, bytes) in VENDOR_FILES {
        let dst = vendor.join("crates").join(rel);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(dst, bytes)?;
    }
    if service {
        for (rel, bytes) in SERVICE_VENDOR_FILES {
            let dst = vendor.join("crates").join(rel);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(dst, bytes)?;
        }
        expand_service_registry_archive(&vendor.join("registry"))?;
        let cargo_config = out_dir.join(".cargo");
        fs::create_dir_all(&cargo_config)?;
        fs::write(cargo_config.join("config.toml"), SERVICE_VENDOR_CONFIG)?;
        fs::write(vendor.join("Cargo.toml"), SERVICE_VENDOR_WORKSPACE_MANIFEST)?;
    } else if lispex_application {
        for (rel, bytes) in LISPEX_APPLICATION_VENDOR_FILES {
            let dst = vendor.join("crates").join(rel);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(dst, bytes)?;
        }
        if full_lispex_application {
            for (rel, bytes) in FULL_LISPEX_APPLICATION_VENDOR_FILES {
                let dst = vendor.join("crates").join(rel);
                if let Some(parent) = dst.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(dst, bytes)?;
            }
        }
        expand_directory_archive(
            LISPEX_APPLICATION_VENDOR_ARCHIVE,
            b"TPZLPXA1",
            &vendor.join("registry"),
            "Lispex application",
        )?;
        let cargo_config = out_dir.join(".cargo");
        fs::create_dir_all(&cargo_config)?;
        fs::write(
            cargo_config.join("config.toml"),
            LISPEX_APPLICATION_VENDOR_CONFIG,
        )?;
        fs::write(
            vendor.join("Cargo.toml"),
            LISPEX_APPLICATION_VENDOR_WORKSPACE_MANIFEST,
        )?;
    } else {
        fs::write(vendor.join("Cargo.toml"), VENDOR_WORKSPACE_MANIFEST)?;
    }
    Ok(())
}

pub(super) fn expand_service_registry_archive(destination: &Path) -> std::io::Result<()> {
    expand_directory_archive(
        SERVICE_VENDOR_ARCHIVE,
        b"TPZHTTP1",
        destination,
        "HTTP service",
    )
}

pub(super) fn expand_directory_archive(
    archive: &[u8],
    magic: &[u8; 8],
    destination: &Path,
    label: &str,
) -> std::io::Result<()> {
    fn invalid(message: &'static str) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidData, message)
    }
    fn take<'a>(archive: &'a [u8], cursor: &mut usize, len: usize) -> std::io::Result<&'a [u8]> {
        let end = cursor
            .checked_add(len)
            .ok_or_else(|| invalid("service vendor archive offset overflow"))?;
        let bytes = archive
            .get(*cursor..end)
            .ok_or_else(|| invalid("truncated service vendor archive"))?;
        *cursor = end;
        Ok(bytes)
    }
    fn take_u32(archive: &[u8], cursor: &mut usize) -> std::io::Result<u32> {
        let bytes: [u8; 4] = take(archive, cursor, 4)?
            .try_into()
            .map_err(|_| invalid("invalid service vendor u32"))?;
        Ok(u32::from_le_bytes(bytes))
    }
    fn take_u64(archive: &[u8], cursor: &mut usize) -> std::io::Result<u64> {
        let bytes: [u8; 8] = take(archive, cursor, 8)?
            .try_into()
            .map_err(|_| invalid("invalid service vendor u64"))?;
        Ok(u64::from_le_bytes(bytes))
    }

    if archive.get(..8) != Some(magic) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid {label} vendor archive magic"),
        ));
    }
    let mut cursor = 8;
    let count = take_u32(archive, &mut cursor)?;
    for _ in 0..count {
        let path_len = usize::try_from(take_u32(archive, &mut cursor)?)
            .map_err(|_| invalid("service vendor path length does not fit usize"))?;
        let data_len = usize::try_from(take_u64(archive, &mut cursor)?)
            .map_err(|_| invalid("service vendor file length does not fit usize"))?;
        let path = std::str::from_utf8(take(archive, &mut cursor, path_len)?)
            .map_err(|_| invalid("service vendor path is not UTF-8"))?;
        let normalized = Path::new(path);
        if normalized.is_absolute()
            || normalized
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(invalid(
                "service vendor archive path is not relative-normal",
            ));
        }
        let bytes = take(archive, &mut cursor, data_len)?;
        let output = destination.join(normalized);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(output, bytes)?;
    }
    if cursor != archive.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{label} vendor archive has trailing bytes"),
        ));
    }
    Ok(())
}

/// Write a self-contained, compilable Cargo crate for the lowered program into
/// `out_dir`: a `Cargo.toml` (relative path deps on the VENDORED `topaz_rt` /
/// `topaz_host_native` under `vendor/`, with its own `[workspace]` so it is not
/// absorbed into the vendor workspace), `src/emitted.rs` (the `emit_module`
/// output), a `src/main.rs` host harness, the pinned `rust-toolchain.toml`, and
/// the vendored runtime closure under `vendor/` (CDR-006 §7). `cargo run` in
/// `out_dir` reproduces the program with no source checkout or registry.
pub(super) fn scaffold_crate(
    out_dir: &Path,
    rust: &str,
    harness: HostHarness<'_>,
) -> std::io::Result<()> {
    scaffold_native_crate(out_dir, rust, harness, false)
}

pub(super) fn scaffold_native_crate(
    out_dir: &Path,
    rust: &str,
    harness: HostHarness<'_>,
    service: bool,
) -> std::io::Result<()> {
    let lispex_application = harness.lispex_application();
    let src = out_dir.join("src");
    fs::create_dir_all(&src)?;
    let mut cargo_toml = "[package]\n\
         name = \"topaz-emitted\"\n\
         version = \"0.0.0\"\n\
         edition = \"2024\"\n\
         # Consume ONLY the declared library and binary: no build script, and no\n\
         # auto-discovery, so a stale file in a reused out-dir (a stray build.rs /\n\
         # src/bin/*) cannot inject an unaudited target into the build.\n\
         build = false\n\
         autolib = false\n\
         autobins = false\n\
         autoexamples = false\n\
         autotests = false\n\
         autobenches = false\n\
         \n\
         [lib]\n\
         name = \"topaz_emitted\"\n\
         path = \"src/emitted.rs\"\n\
         \n\
         [[bin]]\n\
         name = \"program\"\n\
         path = \"src/main.rs\"\n\
         \n\
         # Own workspace; EXCLUDE vendor/ so the vendored closure crates resolve\n\
         # their workspace inheritance against vendor/Cargo.toml, not this crate.\n\
         [workspace]\n\
         exclude = [\"vendor\"]\n\
         \n\
         [dependencies]\n\
         topaz_rt = { path = \"vendor/crates/topaz_rt\" }\n\
         topaz_host_native = { path = \"vendor/crates/topaz_host_native\" }\n"
        .to_string();
    if service {
        cargo_toml.push_str("topaz_host_http = { path = \"vendor/crates/topaz_host_http\" }\n");
    }
    if let Some(application) = lispex_application {
        if application
            .rules
            .first()
            .is_some_and(|rule| rule.identity.profile == topaz_lispex_embed::FULL_PROFILE_ID)
        {
            cargo_toml.push_str(
                "topaz_lispex_embed = { path = \"vendor/crates/topaz_lispex_embed\", default-features = false, features = [\"managed-product-component\", \"full-profile-contract\"] }\n",
            );
        } else {
            cargo_toml.push_str(
                "topaz_lispex_embed = { path = \"vendor/crates/topaz_lispex_embed\", default-features = false, features = [\"managed-product-component\"] }\n",
            );
        }
    }
    fs::write(out_dir.join("Cargo.toml"), cargo_toml)?;
    fs::write(src.join("emitted.rs"), rust)?;
    fs::write(src.join("main.rs"), main_harness(harness))?;
    if let Some(plan) = lispex_application {
        write_lispex_payload(out_dir, plan)?;
        fs::write(
            out_dir.join("lispex/RUNTIME-THIRD-PARTY-NOTICES.txt"),
            LISPEX_APPLICATION_THIRD_PARTY_NOTICES,
        )?;
    }
    // Pin the toolchain the closure was built against (CDR-006 §7), embedded in
    // the binary so a checkout is not required.
    fs::write(out_dir.join("rust-toolchain.toml"), VENDOR_TOOLCHAIN)?;
    write_source_distribution_notices(out_dir)?;
    let full_lispex_application = lispex_application.is_some_and(|plan| {
        plan.rules
            .first()
            .is_some_and(|rule| rule.identity.profile == topaz_lispex_embed::FULL_PROFILE_ID)
    });
    expand_vendor(
        out_dir,
        service,
        lispex_application.is_some(),
        full_lispex_application,
    )?;
    Ok(())
}

fn write_lispex_payload(
    root: &Path,
    plan: &topaz_lispex_product::CheckedApplicationPlan,
) -> std::io::Result<()> {
    if !plan.payload.requires_runtime || plan.payload.files.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "a reached Lispex application plan has no conditional runtime payload",
        ));
    }
    for file in &plan.payload.files {
        let relative = Path::new(&file.path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Lispex payload path is not relative-normal",
            ));
        }
        let destination = root.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(destination, &file.bytes)?;
    }
    Ok(())
}
