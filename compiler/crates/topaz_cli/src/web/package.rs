use crate::*;

pub(super) fn resolve_and_lower_entry_for_web(
    entry: &str,
    root: Option<&str>,
    version: LangVersion,
    backend: Backend,
    native_report: Option<&mut NativeReportSession>,
    command: &'static str,
    build_target: &str,
) -> Result<WebLowered, ExitCode> {
    if version == LangVersion::V5_1 {
        eprintln!(
            "topaz: `build --target web` needs v5.2+ (v5.16 is the default); `--language-version 5.1` has no module system"
        );
        return Err(ExitCode::FAILURE);
    }
    let entry_norm = entry.replace('\\', "/");
    let (base, entry_rel, root_rel) = split_absolute(&entry_norm, root).map_err(|msg| {
        eprintln!("topaz: {msg}");
        ExitCode::FAILURE
    })?;
    let provider = PhysicalProvider::new(base.clone());
    let unit = resolve_with_version(&provider, &entry_rel, root_rel.as_deref(), version);
    lower_web_resolved_unit(
        &unit,
        WebLoweringContext {
            version,
            backend,
            label: &entry_norm,
            package_target: None,
            native_report,
            command,
            build_target,
        },
    )
}

#[cfg(test)]
pub(super) fn resolve_and_lower_package_for_web(
    target: &PackageTarget,
    backend: Backend,
) -> Result<WebLowered, ExitCode> {
    resolve_and_lower_package_for_web_with_report(target, backend, None, "build", "web")
}

pub(super) fn resolve_and_lower_package_for_web_with_report(
    target: &PackageTarget,
    backend: Backend,
    native_report: Option<&mut NativeReportSession>,
    command: &'static str,
    build_target: &str,
) -> Result<WebLowered, ExitCode> {
    let unit = resolve_package_target(target);
    lower_web_resolved_unit(
        &unit,
        WebLoweringContext {
            version: target.version,
            backend,
            label: &target.entry,
            package_target: Some(target),
            native_report,
            command,
            build_target,
        },
    )
}

pub(super) struct WebLoweringContext<'context> {
    pub(super) version: LangVersion,
    pub(super) backend: Backend,
    pub(super) label: &'context str,
    pub(super) package_target: Option<&'context PackageTarget>,
    pub(super) native_report: Option<&'context mut NativeReportSession>,
    pub(super) command: &'static str,
    pub(super) build_target: &'context str,
}

pub(super) fn lower_web_resolved_unit(
    unit: &topaz_resolve::ResolveOutput,
    context: WebLoweringContext<'_>,
) -> Result<WebLowered, ExitCode> {
    let WebLoweringContext {
        version,
        backend,
        label,
        package_target,
        native_report,
        command,
        build_target,
    } = context;
    for diag in &unit.diagnostics {
        eprintln!("{}", render(diag, &unit.map));
    }
    if has_errors(&unit.diagnostics) {
        return Err(ExitCode::FAILURE);
    }
    let checked = match check_resolved_unit(unit, false, version) {
        Ok(checked) => checked,
        Err(n) => {
            eprintln!(
                "{label}: {n} type diagnostic{}",
                if n == 1 { "" } else { "s" }
            );
            return Err(ExitCode::FAILURE);
        }
    };
    if let Some(target) = package_target {
        reject_reached_lispex_application_target(target, &checked, build_target)?;
    }
    let entry_identity = unit
        .modules
        .iter()
        .find(|m| m.is_entry)
        .map(|m| m.identity.as_str())
        .unwrap_or("");
    let entry_exports = checked
        .exports
        .get(entry_identity)
        .cloned()
        .unwrap_or_default();
    let mut byte_buffer_exports = entry_exports
        .values
        .iter()
        .filter_map(|(name, value)| {
            let mut seen = std::collections::BTreeSet::new();
            web_type_contains_byte_buffer(&value.ty, &value.nominals, &mut seen)
                .then_some(name.as_str())
        })
        .collect::<Vec<_>>();
    byte_buffer_exports.sort_unstable();
    if !byte_buffer_exports.is_empty() {
        eprintln!(
            "topaz: Web ABI export{} {} contain{} `ByteBuffer`; snapshot to `Bytes` before the boundary",
            if byte_buffer_exports.len() == 1 {
                ""
            } else {
                "s"
            },
            byte_buffer_exports
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", "),
            if byte_buffer_exports.len() == 1 {
                "s"
            } else {
                ""
            },
        );
        return Err(ExitCode::FAILURE);
    }
    let records = collect_exported_records(&checked.exports, &entry_exports);
    let enums = collect_exported_enums(&checked.exports, &entry_exports);
    let newtypes = collect_exported_newtypes(&checked.exports, &entry_exports);
    let rust = lower_checked_unit_with_report(
        unit,
        version,
        backend,
        Some(&checked),
        native_report,
        command,
        build_target,
    )?;
    let compiler = rust_compiler_provenance(unit, &rust).map_err(|error| {
        eprintln!("topaz: cannot record compiler provenance: {error}");
        ExitCode::FAILURE
    })?;
    Ok(WebLowered {
        rust,
        compiler,
        entry_exports,
        records,
        enums,
        newtypes,
    })
}

pub(super) fn web_type_contains_byte_buffer(
    ty: &topaz_check::Type,
    nominals: &topaz_check::unit::ExportedNominals,
    seen: &mut std::collections::BTreeSet<String>,
) -> bool {
    use topaz_check::Type;
    match ty {
        Type::ByteBuffer => true,
        Type::Union(members) => members
            .iter()
            .any(|member| web_type_contains_byte_buffer(member, nominals, seen)),
        Type::Record(fields) => fields
            .iter()
            .any(|(_, field)| web_type_contains_byte_buffer(field, nominals, seen)),
        Type::Ctor(_, args) | Type::Foreign { args, .. } => args
            .iter()
            .any(|arg| web_type_contains_byte_buffer(arg, nominals, seen)),
        Type::Func {
            params,
            variadic,
            ret,
        } => {
            params
                .iter()
                .any(|param| web_type_contains_byte_buffer(param, nominals, seen))
                || variadic
                    .as_deref()
                    .is_some_and(|param| web_type_contains_byte_buffer(param, nominals, seen))
                || web_type_contains_byte_buffer(ret, nominals, seen)
        }
        Type::NominalRecord { args, .. } => {
            if args
                .iter()
                .any(|arg| web_type_contains_byte_buffer(arg, nominals, seen))
            {
                return true;
            }
            let id = ty.to_string();
            if !seen.insert(format!("record:{id}")) {
                return false;
            }
            let found = nominals
                .records
                .get(&id)
                .or_else(|| nominals.records.values().find(|record| record.id == id))
                .is_some_and(|record| {
                    record
                        .fields
                        .iter()
                        .any(|field| web_type_contains_byte_buffer(&field.ty, nominals, seen))
                });
            seen.remove(&format!("record:{id}"));
            found
        }
        Type::Enum { args, .. } => {
            if args
                .iter()
                .any(|arg| web_type_contains_byte_buffer(arg, nominals, seen))
            {
                return true;
            }
            let id = ty.to_string();
            if !seen.insert(format!("enum:{id}")) {
                return false;
            }
            let found = nominals
                .enums
                .get(&id)
                .or_else(|| nominals.enums.values().find(|enm| enm.id == id))
                .is_some_and(|enm| {
                    enm.variants.iter().any(|variant| {
                        variant
                            .payloads
                            .iter()
                            .any(|payload| web_type_contains_byte_buffer(payload, nominals, seen))
                    })
                });
            seen.remove(&format!("enum:{id}"));
            found
        }
        Type::Newtype { args, .. } => {
            if args
                .iter()
                .any(|arg| web_type_contains_byte_buffer(arg, nominals, seen))
            {
                return true;
            }
            let id = ty.to_string();
            if !seen.insert(format!("newtype:{id}")) {
                return false;
            }
            let found = nominals
                .newtypes
                .get(&id)
                .or_else(|| nominals.newtypes.values().find(|newtype| newtype.id == id))
                .is_some_and(|newtype| {
                    web_type_contains_byte_buffer(&newtype.base, nominals, seen)
                });
            seen.remove(&format!("newtype:{id}"));
            found
        }
        Type::Prim(_)
        | Type::Literal(_)
        | Type::Skolem { .. }
        | Type::Template
        | Type::File
        | Type::JsonValue
        | Type::Bytes
        | Type::Path
        | Type::Regex
        | Type::Match
        | Type::TomlValue
        | Type::Url
        | Type::Date
        | Type::BigInt
        | Type::Decimal
        | Type::RoundingMode
        | Type::Unknown
        | Type::Var(_) => false,
    }
}

/// Write the v5.4 Web Target PW1 crate: same emitted Rust semantics, packaged
/// as a raw `wasm32-unknown-unknown` cdylib plus a zero-dependency JS loader.
/// JS is glue only; all Topaz values cross as canonical ABI JSON.
pub(super) fn scaffold_web_crate(
    out_dir: &Path,
    rust: &str,
    dts: &str,
    write_worker: bool,
) -> std::io::Result<()> {
    let src = out_dir.join("src");
    fs::create_dir_all(&src)?;
    let cargo_toml = "[package]\n\
         name = \"topaz-emitted-web\"\n\
         version = \"0.0.0\"\n\
         edition = \"2024\"\n\
         build = false\n\
         autolib = false\n\
         autobins = false\n\
         autoexamples = false\n\
         autotests = false\n\
         autobenches = false\n\
         \n\
         [lib]\n\
         name = \"topaz_emitted_web\"\n\
         path = \"src/lib.rs\"\n\
         crate-type = [\"cdylib\"]\n\
         \n\
         [workspace]\n\
         exclude = [\"vendor\"]\n\
         \n\
         [dependencies]\n\
         topaz_rt = { path = \"vendor/crates/topaz_rt\" }\n\
         \n\
         [profile.release]\n\
         opt-level = \"s\"\n\
         lto = true\n";
    fs::write(out_dir.join("Cargo.toml"), cargo_toml)?;
    fs::write(src.join("emitted.rs"), rust)?;
    fs::write(src.join("lib.rs"), WEB_LIB_RS)?;
    let loader = format!(
        "export const TOPAZ_TOOLCHAIN_VERSION = {:?};\n{}",
        env!("CARGO_PKG_VERSION"),
        WEB_LOADER_JS
    );
    fs::write(out_dir.join("topaz-web.js"), loader)?;
    fs::write(out_dir.join("topaz-web.d.ts"), dts)?;
    if write_worker {
        fs::write(out_dir.join("topaz-web-worker.js"), WEB_WORKER_JS)?;
        fs::write(
            out_dir.join("topaz-web-worker-client.js"),
            WEB_WORKER_CLIENT_JS,
        )?;
    }
    fs::write(out_dir.join("rust-toolchain.toml"), VENDOR_TOOLCHAIN)?;
    write_source_distribution_notices(out_dir)?;
    expand_vendor(out_dir, false, false, false)?;
    Ok(())
}

pub(super) fn write_source_distribution_notices(out_dir: &Path) -> std::io::Result<()> {
    fs::write(out_dir.join("LICENSE-RUNTIME"), artifact::license_text())?;
    fs::write(out_dir.join("NOTICE"), artifact::notice_text())?;
    fs::write(
        out_dir.join(artifact::OUTPUT_NOTICE_NAME),
        artifact::output_notice_text(),
    )
}

pub(super) struct WebPackageBuild<'a> {
    pub(super) dir: &'a Path,
    pub(super) rust: &'a str,
    pub(super) compiler: artifact::CompilerProvenance,
    pub(super) release: bool,
    pub(super) label: &'a str,
    pub(super) entry_exports: &'a topaz_check::ModuleExports,
    pub(super) records: &'a ExportedRecords,
    pub(super) enums: &'a ExportedEnums,
    pub(super) newtypes: &'a ExportedNewtypes,
    pub(super) language_version: LangVersion,
    pub(super) target: BuildTarget,
    pub(super) package_root: Option<&'a Path>,
    pub(super) package_name: Option<&'a str>,
    pub(super) web: Option<&'a topaz_package::WebConfig>,
    pub(super) web_capabilities: Option<&'a topaz_package::WebCapabilities>,
}

pub(super) fn build_web_package(input: WebPackageBuild<'_>) -> ExitCode {
    let WebPackageBuild {
        dir,
        rust,
        compiler,
        release,
        label,
        entry_exports,
        records,
        enums,
        newtypes,
        language_version,
        target,
        package_root,
        package_name,
        web,
        web_capabilities,
    } = input;
    let artifact_target = if target == BuildTarget::WebApp {
        artifact::Target::WebApp
    } else if target.writes_worker() {
        artifact::Target::WebWorker
    } else {
        artifact::Target::Web
    };
    let destination = match artifact::Destination::open(dir, artifact_target) {
        Ok(destination) => destination,
        Err(e) => {
            eprintln!("topaz: cannot use output directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let workspace = match storage::Workspace::create() {
        Ok(workspace) => workspace,
        Err(e) => {
            eprintln!("topaz: cannot create web build workspace: {e}");
            return ExitCode::FAILURE;
        }
    };
    let dts = render_web_types(entry_exports, records, enums, newtypes);
    if let Err(e) = scaffold_web_crate(&workspace.source, rust, &dts, target.writes_worker()) {
        eprintln!("topaz: could not scaffold the temporary web package: {e}");
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
    eprintln!(
        "topaz: compiling `{label}` for {} (wasm32, offline, locked, isolated) …",
        target.label()
    );
    let mut cmd = env.cargo();
    cmd.args([
        "build",
        "--offline",
        "--locked",
        "--target",
        "wasm32-unknown-unknown",
        "--manifest-path",
    ])
    .arg(&env.manifest);
    if release {
        cmd.arg("--release");
    }
    if let Err(code) = run_cargo_logged(&env, "web-build", cmd) {
        return code;
    }
    let built_wasm = env
        .target
        .join("wasm32-unknown-unknown")
        .join(profile)
        .join("topaz_emitted_web.wasm");
    let mut files = Vec::new();
    for name in ["topaz-web.js", "topaz-web.d.ts"] {
        match fs::read(env.workspace.source.join(name)) {
            Ok(bytes) => files.push(artifact::File::binary(name, bytes, false)),
            Err(e) => {
                eprintln!("topaz: temporary web output `{name}` is missing: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    if target.writes_worker() {
        for name in ["topaz-web-worker.js", "topaz-web-worker-client.js"] {
            match fs::read(env.workspace.source.join(name)) {
                Ok(bytes) => files.push(artifact::File::binary(name, bytes, false)),
                Err(e) => {
                    eprintln!("topaz: temporary web output `{name}` is missing: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
    }
    let wasm = match fs::read(&built_wasm) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("topaz: web build succeeded but its final WASM is missing: {e}");
            return ExitCode::FAILURE;
        }
    };
    files.push(artifact::File::binary("topaz-web.wasm", wasm, false));
    if target == BuildTarget::WebApp {
        let (Some(package_root), Some(package_name), Some(web), Some(web_capabilities)) =
            (package_root, package_name, web, web_capabilities)
        else {
            eprintln!("topaz: internal web-app build is missing package metadata");
            return ExitCode::FAILURE;
        };
        match web_app_files(package_root, package_name, web, web_capabilities) {
            Ok(app_files) => files.extend(app_files),
            Err(error) => {
                eprintln!("topaz: cannot assemble web-app package: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    let plan = artifact::Plan {
        target: artifact_target,
        language_version,
        entry: logical_entry(label),
        runtime_requirements: vec!["WebAssembly and an ES module host".into()],
        invocation: if target == BuildTarget::WebApp {
            "serve this directory and open index.html".into()
        } else {
            "import { instantiateTopaz } from './topaz-web.js'".into()
        },
        compiler: Some(compiler),
        files,
    };
    if let Err(e) = destination.commit(plan) {
        eprintln!(
            "topaz: could not install web artifact in `{}`: {e}",
            dir.display()
        );
        return ExitCode::FAILURE;
    }
    eprintln!(
        "topaz: built {} package `{}` (loader: `{}`)",
        target.label(),
        dir.join("topaz-web.wasm").display(),
        dir.join("topaz-web.js").display()
    );
    ExitCode::SUCCESS
}

pub(super) fn web_app_files(
    package_root: &Path,
    package_name: &str,
    web: &topaz_package::WebConfig,
    web_capabilities: &topaz_package::WebCapabilities,
) -> Result<Vec<artifact::File>, String> {
    let host = WEB_APP_JS
        .replace("__TOPAZ_TOOLCHAIN_VERSION__", env!("CARGO_PKG_VERSION"))
        .replace("__TOPAZ_WEB_LIFECYCLE__", web.lifecycle.as_str())
        .replace(
            "__TOPAZ_OPEN_TEXT__",
            if web_capabilities.open_text {
                "true"
            } else {
                "false"
            },
        )
        .replace(
            "__TOPAZ_DOWNLOAD_TEXT__",
            if web_capabilities.download_text {
                "true"
            } else {
                "false"
            },
        )
        .replace(
            "__TOPAZ_LOCAL_STATE__",
            if web_capabilities.local_state {
                "true"
            } else {
                "false"
            },
        )
        .replace(
            "__TOPAZ_STATE_NAMESPACE__",
            &format!("topaz.web-state.v1:{package_name}:"),
        )
        .replace(
            "__TOPAZ_MAX_TEXT_BYTES__",
            &WEB_APP_MAX_TEXT_BYTES.to_string(),
        )
        .replace(
            "__TOPAZ_MAX_LIVE_REQUESTS__",
            &WEB_APP_MAX_LIVE_REQUESTS.to_string(),
        )
        .replace(
            "__TOPAZ_MAX_STATE_VALUE_BYTES__",
            &WEB_APP_MAX_STATE_VALUE_BYTES.to_string(),
        )
        .replace(
            "__TOPAZ_MAX_STATE_KEYS__",
            &WEB_APP_MAX_STATE_KEYS.to_string(),
        );
    let mut files = vec![
        artifact::File::text("index.html", web_app_index(web)),
        artifact::File::text("topaz-app.js", host),
        artifact::File::text(
            "topaz-web-capabilities.json",
            web_capabilities_json(package_name, web, web_capabilities),
        ),
    ];
    let mut seen = std::collections::BTreeSet::new();
    for path in web.styles.iter().chain(&web.assets) {
        collect_web_input(package_root, Path::new(path), &mut seen, &mut files)?;
    }
    Ok(files)
}
