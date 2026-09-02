use super::*;
use crate::*;

pub(crate) fn parse_package_section(
    table: &BTreeMap<Rc<str>, TomlValue>,
) -> Result<PackageSection, PackageError> {
    reject_unknown_keys(
        table,
        "[package]",
        &["name", "version", "language", "entry", "license"],
    )?;
    let name = required_string(table, "name", "[package]")?;
    validate_package_name(&name)?;
    let version = required_string(table, "version", "[package]")?;
    validate_package_version("[package].version", &version)?;
    let language = parse_language(&required_string(table, "language", "[package]")?)?;
    if language == LangVersion::V5_1 {
        return Err(PackageError::new(
            "[package].language must be 5.2 or newer for package mode",
        ));
    }
    let entry = normalize_project_path(
        "[package].entry",
        &required_string(table, "entry", "[package]")?,
    )?;
    let license = optional_string(table, "license", "[package]")?;
    Ok(PackageSection {
        name,
        version,
        language,
        entry,
        license,
    })
}

pub(crate) fn parse_build_section(
    table: Option<&BTreeMap<Rc<str>, TomlValue>>,
    retain_nondeterministic: bool,
) -> Result<BuildConfig, PackageError> {
    if let Some(table) = table {
        reject_unknown_keys(table, "[build]", &["target", "deterministic"])?;
    }
    let target = match table {
        Some(t) => optional_string(t, "target", "[build]")?.unwrap_or_else(|| "native".to_string()),
        None => "native".to_string(),
    };
    if target.trim().is_empty() {
        return Err(PackageError::new("[build].target must be non-empty"));
    }
    if ![
        "native",
        "web",
        "web-worker",
        "web-app",
        "http-service",
        "python",
    ]
    .contains(&target.as_str())
    {
        return Err(PackageError::new(format!(
            "[build].target `{target}` is unsupported (expected `native`, `web`, `web-worker`, `web-app`, `http-service`, or `python`)"
        )));
    }
    let deterministic = match table {
        Some(t) => optional_bool(t, "deterministic", "[build]")?.unwrap_or(true),
        None => true,
    };
    if !deterministic && !retain_nondeterministic {
        return Err(PackageError::new(
            "[build].deterministic must be true for v5.4 package mode",
        ));
    }
    Ok(BuildConfig {
        target,
        deterministic,
    })
}

pub(crate) fn parse_web_section(
    table: Option<&BTreeMap<Rc<str>, TomlValue>>,
) -> Result<WebConfig, PackageError> {
    let Some(table) = table else {
        return Ok(WebConfig::default());
    };
    reject_unknown_keys(table, "[web]", &["title", "styles", "assets", "lifecycle"])?;
    let title = optional_string(table, "title", "[web]")?
        .unwrap_or_else(|| "Topaz application".to_string());
    if title.trim().is_empty() {
        return Err(PackageError::new("[web].title must be non-empty"));
    }
    let lifecycle = match optional_string(table, "lifecycle", "[web]")?.as_deref() {
        None | Some("v1") => WebLifecycle::V1,
        Some("v2") => WebLifecycle::V2,
        Some(other) => {
            return Err(PackageError::new(format!(
                "[web].lifecycle `{other}` is unsupported (expected `v1` or `v2`)"
            )));
        }
    };
    let styles = optional_string_array(table, "styles", "[web]")?
        .unwrap_or_default()
        .into_iter()
        .map(|path| normalize_project_path("[web].styles", &path))
        .collect::<Result<Vec<_>, _>>()?;
    let assets = optional_string_array(table, "assets", "[web]")?
        .unwrap_or_default()
        .into_iter()
        .map(|path| normalize_project_path("[web].assets", &path))
        .collect::<Result<Vec<_>, _>>()?;
    for path in &styles {
        if !path.starts_with("styles/") || !path.ends_with(".css") {
            return Err(PackageError::new(format!(
                "[web].styles path `{path}` must be a `.css` file under `styles/`"
            )));
        }
    }
    for path in &assets {
        if !path.starts_with("assets/") {
            return Err(PackageError::new(format!(
                "[web].assets path `{path}` must be under `assets/`"
            )));
        }
    }
    let mut declared = BTreeSet::new();
    for path in styles.iter().chain(&assets) {
        if path
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '?' | '#'))
        {
            return Err(PackageError::new(format!(
                "[web] path `{path}` must not contain a query, fragment, or control character"
            )));
        }
        if !declared.insert(path.clone()) {
            return Err(PackageError::new(format!(
                "[web] path `{path}` is declared more than once"
            )));
        }
    }
    Ok(WebConfig {
        title,
        styles,
        assets,
        lifecycle,
    })
}

pub(crate) fn parse_service_section(
    table: Option<&BTreeMap<Rc<str>, TomlValue>>,
) -> Result<ServiceConfig, PackageError> {
    let Some(table) = table else {
        return Ok(ServiceConfig::default());
    };
    reject_unknown_keys(
        table,
        "[service]",
        &[
            "bind",
            "port",
            "workers",
            "max_connections",
            "queue_capacity",
            "max_target_bytes",
            "max_header_bytes",
            "max_headers",
            "max_body_bytes",
            "header_timeout_ms",
            "body_timeout_ms",
            "handler_timeout_ms",
            "shutdown_grace_ms",
            "log_format",
        ],
    )?;
    let defaults = ServiceConfig::default();
    let bind = optional_string(table, "bind", "[service]")?.unwrap_or(defaults.bind);
    if bind != "127.0.0.1" && bind != "::1" {
        return Err(PackageError::new(
            "[service].bind must be a loopback IP literal (`127.0.0.1` or `::1`)",
        ));
    }
    let log_format = match optional_string(table, "log_format", "[service]")?.as_deref() {
        None | Some("text") => ServiceLogFormat::Text,
        Some("json") => ServiceLogFormat::Json,
        Some("off") => ServiceLogFormat::Off,
        Some(other) => {
            return Err(PackageError::new(format!(
                "[service].log_format `{other}` is unsupported (expected `text`, `json`, or `off`)"
            )));
        }
    };
    Ok(ServiceConfig {
        bind,
        port: service_u16(table, "port", defaults.port, 1, u16::MAX)?,
        workers: service_u16(table, "workers", defaults.workers, 1, 64)?,
        max_connections: service_u16(table, "max_connections", defaults.max_connections, 1, 4_096)?,
        queue_capacity: service_u16(table, "queue_capacity", defaults.queue_capacity, 0, 4_096)?,
        max_target_bytes: service_u32(
            table,
            "max_target_bytes",
            defaults.max_target_bytes,
            256,
            16_384,
        )?,
        max_header_bytes: service_u32(
            table,
            "max_header_bytes",
            defaults.max_header_bytes,
            1_024,
            65_536,
        )?,
        max_headers: service_u16(table, "max_headers", defaults.max_headers, 1, 128)?,
        max_body_bytes: service_u32(
            table,
            "max_body_bytes",
            defaults.max_body_bytes,
            0,
            16_777_216,
        )?,
        header_timeout_ms: service_u32(
            table,
            "header_timeout_ms",
            defaults.header_timeout_ms,
            100,
            60_000,
        )?,
        body_timeout_ms: service_u32(
            table,
            "body_timeout_ms",
            defaults.body_timeout_ms,
            100,
            60_000,
        )?,
        handler_timeout_ms: service_u32(
            table,
            "handler_timeout_ms",
            defaults.handler_timeout_ms,
            10,
            60_000,
        )?,
        shutdown_grace_ms: service_u32(
            table,
            "shutdown_grace_ms",
            defaults.shutdown_grace_ms,
            0,
            60_000,
        )?,
        log_format,
    })
}

pub(crate) fn service_u16(
    table: &BTreeMap<Rc<str>, TomlValue>,
    key: &str,
    default: u16,
    min: u16,
    max: u16,
) -> Result<u16, PackageError> {
    let value = service_integer(
        table,
        key,
        i64::from(default),
        i64::from(min),
        i64::from(max),
    )?;
    Ok(value as u16)
}

pub(crate) fn service_u32(
    table: &BTreeMap<Rc<str>, TomlValue>,
    key: &str,
    default: u32,
    min: u32,
    max: u32,
) -> Result<u32, PackageError> {
    let value = service_integer(
        table,
        key,
        i64::from(default),
        i64::from(min),
        i64::from(max),
    )?;
    Ok(value as u32)
}

pub(crate) fn service_integer(
    table: &BTreeMap<Rc<str>, TomlValue>,
    key: &str,
    default: i64,
    min: i64,
    max: i64,
) -> Result<i64, PackageError> {
    let value = match table.get(key) {
        None => default,
        Some(TomlValue::Integer(value)) => *value,
        Some(_) => {
            return Err(PackageError::new(format!(
                "[service].{key} must be an integer"
            )));
        }
    };
    if !(min..=max).contains(&value) {
        return Err(PackageError::new(format!(
            "[service].{key} must be in {min}..={max} (got {value})"
        )));
    }
    Ok(value)
}

pub(crate) fn parse_dependencies(
    table: Option<&BTreeMap<Rc<str>, TomlValue>>,
) -> Result<BTreeMap<String, Dependency>, PackageError> {
    let mut out = BTreeMap::new();
    let Some(table) = table else {
        return Ok(out);
    };
    for (name, value) in table {
        validate_package_name(name)?;
        let dep = match value {
            TomlValue::String(version) => Dependency {
                version: {
                    validate_package_version(&format!("[dependencies].{name}"), version)?;
                    Some(version.to_string())
                },
                path: None,
                hash: None,
            },
            TomlValue::Table(t) => {
                reject_unknown_keys(
                    t,
                    &format!("[dependencies].{name}"),
                    &["version", "path", "hash"],
                )?;
                let version = optional_string(t, "version", "[dependencies]")?;
                let path = optional_string(t, "path", "[dependencies]")?;
                let hash = optional_string(t, "hash", "[dependencies]")?;
                if version.is_none() && path.is_none() {
                    return Err(PackageError::new(format!(
                        "[dependencies].{name} must include `version` or `path`"
                    )));
                }
                if version.is_some() && path.is_some() {
                    return Err(PackageError::new(format!(
                        "[dependencies].{name} must include exactly one of `version` or `path`"
                    )));
                }
                if let Some(version) = &version {
                    validate_package_version(&format!("[dependencies].{name}.version"), version)?;
                }
                if let Some(path) = &path {
                    validate_local_dep_path(name, path)?;
                    let Some(hash) = &hash else {
                        return Err(PackageError::new(format!(
                            "[dependencies].{name} with `path` must include a content `hash`"
                        )));
                    };
                    validate_sha256_hash(&format!("[dependencies].{name}.hash"), hash)?;
                }
                if let Some(hash) = &hash {
                    validate_sha256_hash(&format!("[dependencies].{name}.hash"), hash)?;
                }
                Dependency {
                    version,
                    path,
                    hash,
                }
            }
            _ => {
                return Err(PackageError::new(format!(
                    "[dependencies].{name} must be a version string or inline table"
                )));
            }
        };
        out.insert(name.to_string(), dep);
    }
    Ok(out)
}

pub(crate) fn parse_lispex_section(
    table: &BTreeMap<Rc<str>, TomlValue>,
) -> Result<LispexConfig, PackageError> {
    reject_unknown_keys(
        table,
        "[lispex]",
        &["profile", "application", "application_quotas", "rule"],
    )?;
    let profile = required_string(table, "profile", "[lispex]")?;
    if !matches!(
        profile.as_str(),
        LISPEX_BOUNDED_PROFILE_ID | LISPEX_COMPLETE_PROFILE_ID
    ) {
        return Err(PackageError::new(format!(
            "[lispex].profile `{profile}` is unsupported"
        )));
    }
    let application = optional_string(table, "application", "[lispex]")?;
    if let Some(application) = &application
        && !matches!(
            application.as_str(),
            LISPEX_APPLICATION_PROFILE_ID | LISPEX_COMPLETE_APPLICATION_PROFILE_ID
        )
    {
        return Err(PackageError::new(format!(
            "[lispex].application `{application}` is unsupported"
        )));
    }
    match (profile.as_str(), application.as_deref()) {
        (LISPEX_BOUNDED_PROFILE_ID, Some(LISPEX_COMPLETE_APPLICATION_PROFILE_ID)) => {
            return Err(PackageError::new(format!(
                "[lispex].application `{LISPEX_COMPLETE_APPLICATION_PROFILE_ID}` requires [lispex].profile `{LISPEX_COMPLETE_PROFILE_ID}`"
            )));
        }
        (LISPEX_COMPLETE_PROFILE_ID, Some(LISPEX_APPLICATION_PROFILE_ID) | None) => {
            return Err(PackageError::new(format!(
                "[lispex].profile `{LISPEX_COMPLETE_PROFILE_ID}` requires [lispex].application `{LISPEX_COMPLETE_APPLICATION_PROFILE_ID}`"
            )));
        }
        _ => {}
    }
    let application_quotas = optional_string(table, "application_quotas", "[lispex]")?
        .map(|path| normalize_project_path("[lispex].application_quotas", &path))
        .transpose()?;
    if application.is_some() != application_quotas.is_some() {
        return Err(PackageError::new(
            "[lispex].application and [lispex].application_quotas must appear together",
        ));
    }
    let rules_value = table
        .get("rule")
        .ok_or_else(|| PackageError::new("[lispex] requires at least one [[lispex.rule]]"))?;
    let TomlValue::Array(items) = rules_value else {
        return Err(PackageError::new(
            "[lispex].rule must be an array of [[lispex.rule]] tables",
        ));
    };
    if items.is_empty() {
        return Err(PackageError::new(
            "[lispex] requires at least one [[lispex.rule]]",
        ));
    }
    let mut seen = BTreeSet::new();
    let mut rules = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let context = format!("[[lispex.rule]][{index}]");
        let rule = expect_table(item, &context)?;
        reject_unknown_keys(rule, &context, &["name", "source", "limits"])?;
        let name = required_string(rule, "name", &context)?;
        validate_lispex_rule_name(&format!("{context}.name"), &name)?;
        if !seen.insert(name.clone()) {
            return Err(PackageError::new(format!(
                "[lispex]: duplicate rule name `{name}`"
            )));
        }
        if application.is_some() && LISPEX_APPLICATION_EXPORTS.contains(&name.as_str()) {
            return Err(PackageError::new(format!(
                "{context}.name `{name}` conflicts with a reserved std.lispex export"
            )));
        }
        let source = normalize_project_path(
            &format!("{context}.source"),
            &required_string(rule, "source", &context)?,
        )?;
        let limits = normalize_project_path(
            &format!("{context}.limits"),
            &required_string(rule, "limits", &context)?,
        )?;
        if source == limits {
            return Err(PackageError::new(format!(
                "{context}: source and limits must name different files"
            )));
        }
        rules.push(LispexRule {
            name,
            source,
            limits,
        });
    }
    Ok(LispexConfig {
        profile,
        application,
        application_quotas,
        rules,
    })
}

pub(crate) fn parse_capabilities(
    table: Option<&BTreeMap<Rc<str>, TomlValue>>,
) -> Result<Capabilities, PackageError> {
    let mut out = Capabilities::default();
    let Some(table) = table else {
        return Ok(out);
    };
    reject_unknown_keys(table, "[capabilities]", &["fs", "web"])?;
    if let Some(fs) = optional_table(table, "fs")? {
        reject_unknown_keys(fs, "[capabilities.fs]", &["read", "write"])?;
        out.fs.read = optional_string_array(fs, "read", "[capabilities.fs]")?
            .unwrap_or_default()
            .into_iter()
            .map(|p| normalize_project_path("[capabilities.fs].read", &p))
            .collect::<Result<Vec<_>, _>>()?;
        out.fs.write = optional_string_array(fs, "write", "[capabilities.fs]")?
            .unwrap_or_default()
            .into_iter()
            .map(|p| normalize_project_path("[capabilities.fs].write", &p))
            .collect::<Result<Vec<_>, _>>()?;
    }
    if let Some(web) = optional_table(table, "web")? {
        reject_unknown_keys(
            web,
            "[capabilities.web]",
            &["open_text", "download_text", "local_state"],
        )?;
        out.web.open_text = optional_bool(web, "open_text", "[capabilities.web]")?.unwrap_or(false);
        out.web.download_text =
            optional_bool(web, "download_text", "[capabilities.web]")?.unwrap_or(false);
        out.web.local_state =
            optional_bool(web, "local_state", "[capabilities.web]")?.unwrap_or(false);
    }
    Ok(out)
}

pub(crate) fn parse_exports(table: &BTreeMap<Rc<str>, TomlValue>) -> Result<Exports, PackageError> {
    reject_unknown_keys(table, "[exports]", &["module"])?;
    let module = normalize_project_path(
        "[exports].module",
        &required_string(table, "module", "[exports]")?,
    )?;
    Ok(Exports { module })
}

pub(crate) fn parse_externs(
    table: Option<&BTreeMap<Rc<str>, TomlValue>>,
) -> Result<BTreeMap<String, ExternModule>, PackageError> {
    let mut out = BTreeMap::new();
    let Some(table) = table else {
        return Ok(out);
    };
    let mut path = Vec::new();
    collect_extern_modules(table, &mut path, &mut out)?;
    if out.is_empty() {
        return Err(PackageError::new(
            "[extern] must declare at least one extern module",
        ));
    }
    Ok(out)
}

pub(crate) fn collect_extern_modules(
    table: &BTreeMap<Rc<str>, TomlValue>,
    path: &mut Vec<String>,
    out: &mut BTreeMap<String, ExternModule>,
) -> Result<(), PackageError> {
    let context = extern_context(path);
    if is_extern_module_table(table) {
        if path.is_empty() {
            return Err(PackageError::new(
                "[extern] must use a dotted extern module table",
            ));
        }
        reject_unknown_keys(
            table,
            &context,
            &[
                "hash",
                "abi_hash",
                "functions",
                "artifact",
                "sandbox",
                "replay",
            ],
        )?;
        let name = path.join(".");
        if out.contains_key(&name) {
            return Err(PackageError::new(format!(
                "{context}: duplicate extern module `{name}`"
            )));
        }
        out.insert(name, parse_extern_module(&context, table)?);
        return Ok(());
    }

    if table.is_empty() {
        return Err(PackageError::new(format!(
            "{context} must contain extern module tables"
        )));
    }
    for (segment, value) in table {
        let segment = segment.as_ref();
        let field = if path.is_empty() {
            format!("[extern].{segment}")
        } else {
            format!("{}.{}", context, segment)
        };
        validate_extern_identifier(&field, segment)?;
        if path.is_empty() && matches!(segment, "std" | "topaz") {
            return Err(PackageError::new(format!(
                "[extern].{segment} is reserved for built-in modules"
            )));
        }
        let TomlValue::Table(child) = value else {
            return Err(PackageError::new(format!("{field} must be a table")));
        };
        path.push(segment.to_string());
        collect_extern_modules(child, path, out)?;
        path.pop();
    }
    Ok(())
}

pub(crate) fn parse_extern_module(
    context: &str,
    table: &BTreeMap<Rc<str>, TomlValue>,
) -> Result<ExternModule, PackageError> {
    let hash = required_string(table, "hash", context)?;
    validate_sha256_hash(&format!("{context}.hash"), &hash)?;
    let abi_hash = required_string(table, "abi_hash", context)?;
    validate_sha256_hash(&format!("{context}.abi_hash"), &abi_hash)?;
    let functions = parse_extern_functions(
        context,
        table
            .get("functions")
            .ok_or_else(|| PackageError::new(format!("{context}: missing `functions`")))?,
    )?;
    let replay = parse_extern_replay(
        context,
        table
            .get("replay")
            .ok_or_else(|| PackageError::new(format!("{context}: missing `[replay]`")))?,
    )?;
    let artifact = optional_table(table, "artifact")?
        .map(|artifact| parse_extern_artifact(context, artifact))
        .transpose()?;
    let sandbox = parse_extern_sandbox(context, required_table(table, "sandbox", context)?)?;
    if sandbox.kind == ExternSandboxKind::Wasm && artifact.is_none() {
        return Err(PackageError::new(format!(
            "{context}: sandbox kind `wasm` requires `[artifact]`"
        )));
    }
    Ok(ExternModule {
        hash,
        abi_hash,
        functions,
        artifact,
        sandbox,
        replay,
    })
}

pub(crate) fn parse_extern_functions(
    context: &str,
    value: &TomlValue,
) -> Result<Vec<ExternFunction>, PackageError> {
    let TomlValue::Array(items) = value else {
        return Err(PackageError::new(format!(
            "{context}.functions must be an array of tables"
        )));
    };
    if items.is_empty() {
        return Err(PackageError::new(format!(
            "{context}.functions must not be empty"
        )));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        let item_context = format!("{context}.functions[{idx}]");
        let table = expect_table(item, &item_context)?;
        reject_unknown_keys(table, &item_context, &["name", "params", "result"])?;
        let name = required_string(table, "name", &item_context)?;
        validate_extern_identifier(&format!("{item_context}.name"), &name)?;
        if !seen.insert(name.clone()) {
            return Err(PackageError::new(format!(
                "{context}: duplicate extern function `{name}`"
            )));
        }
        let params = required_string_array(table, "params", &item_context)?
            .into_iter()
            .enumerate()
            .map(|(param_idx, raw)| {
                parse_abi_type_field(&format!("{item_context}.params[{param_idx}]"), raw.as_str())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let result = parse_abi_type_field(
            &format!("{item_context}.result"),
            &required_string(table, "result", &item_context)?,
        )?;
        out.push(ExternFunction {
            name,
            params,
            result,
        });
    }
    Ok(out)
}

pub(crate) fn parse_extern_replay(
    context: &str,
    value: &TomlValue,
) -> Result<ExternReplay, PackageError> {
    let table = expect_table(value, &format!("{context}.replay"))?;
    reject_unknown_keys(table, &format!("{context}.replay"), &["fixture"])?;
    let fixture = normalize_project_path(
        &format!("{context}.replay.fixture"),
        &required_string(table, "fixture", &format!("{context}.replay"))?,
    )?;
    Ok(ExternReplay { fixture })
}

pub(crate) fn parse_extern_artifact(
    context: &str,
    table: &BTreeMap<Rc<str>, TomlValue>,
) -> Result<ExternArtifact, PackageError> {
    let artifact_context = format!("{context}.artifact");
    reject_unknown_keys(table, &artifact_context, &["path"])?;
    let path = normalize_project_path(
        &format!("{artifact_context}.path"),
        &required_string(table, "path", &artifact_context)?,
    )?;
    Ok(ExternArtifact { path })
}

pub(crate) fn parse_extern_sandbox(
    context: &str,
    table: &BTreeMap<Rc<str>, TomlValue>,
) -> Result<ExternSandbox, PackageError> {
    let sandbox_context = format!("{context}.sandbox");
    reject_unknown_keys(table, &sandbox_context, &["kind", "fuel", "memory_bytes"])?;
    let kind = parse_extern_sandbox_kind_field(
        &format!("{sandbox_context}.kind"),
        &required_string(table, "kind", &sandbox_context)?,
    )?;
    let fuel = optional_u64(table, "fuel", &sandbox_context)?;
    let memory_bytes = optional_u64(table, "memory_bytes", &sandbox_context)?;
    Ok(ExternSandbox {
        kind,
        fuel,
        memory_bytes,
    })
}

pub(crate) fn parse_extern_sandbox_kind_field(
    field: &str,
    raw: &str,
) -> Result<ExternSandboxKind, PackageError> {
    match raw {
        "replay" => Ok(ExternSandboxKind::Replay),
        "wasm" => Ok(ExternSandboxKind::Wasm),
        other => Err(PackageError::new(format!(
            "{field} must be `replay` or `wasm` (got `{other}`)"
        ))),
    }
}

pub(crate) fn is_extern_module_table(table: &BTreeMap<Rc<str>, TomlValue>) -> bool {
    // Marker keys intentionally stop namespace descent. A namespace segment
    // literally named as one of these keys is rejected conservatively instead
    // of being silently interpreted as a child module.
    [
        "hash",
        "abi_hash",
        "functions",
        "artifact",
        "sandbox",
        "replay",
    ]
    .iter()
    .any(|key| table.contains_key(*key))
}

pub(crate) fn extern_context(path: &[String]) -> String {
    if path.is_empty() {
        "[extern]".to_string()
    } else {
        format!("[extern.{}]", path.join("."))
    }
}

pub(crate) fn parse_language(raw: &str) -> Result<LangVersion, PackageError> {
    match LangVersion::parse_exact(raw) {
        Some(version) if version.is_selectable() => Ok(version),
        Some(_) => Err(PackageError::new(format!(
            "[package].language `{raw}` is known but not current in this toolchain"
        ))),
        None => Err(PackageError::new(format!(
            "[package].language `{raw}` is not supported"
        ))),
    }
}

pub(crate) fn validate_package_name(name: &str) -> Result<(), PackageError> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(PackageError::new(format!(
            "package/dependency name `{name}` must contain only ASCII letters, digits, `_`, or `-`"
        )));
    }
    Ok(())
}

pub(crate) fn validate_package_version(field: &str, version: &str) -> Result<(), PackageError> {
    if version.trim().is_empty() {
        return Err(PackageError::new(format!("{field} must be non-empty")));
    }
    if !version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
    {
        return Err(PackageError::new(format!(
            "{field} must contain only ASCII letters, digits, `.`, `_`, `-`, or `+`"
        )));
    }
    Ok(())
}

pub(crate) fn validate_extern_identifier(field: &str, name: &str) -> Result<(), PackageError> {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return Err(PackageError::new(format!("{field} must be non-empty")));
    };
    if !is_ident_start(first) || !bytes.all(is_ident_continue) {
        return Err(PackageError::new(format!(
            "{field} `{name}` must be a Topaz identifier using ASCII letters, digits, or `_`"
        )));
    }
    Ok(())
}

pub(crate) fn validate_lispex_rule_name(field: &str, name: &str) -> Result<(), PackageError> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(PackageError::new(format!("{field} must be non-empty")));
    };
    if !is_identifier_start(first) || !chars.all(is_identifier_continue) {
        return Err(PackageError::new(format!(
            "{field} `{name}` must be one canonical Topaz identifier"
        )));
    }
    if Keyword::lookup(name).is_some() {
        return Err(PackageError::new(format!(
            "{field} `{name}` is a reserved Topaz keyword"
        )));
    }
    Ok(())
}

pub(crate) fn validate_extern_module_name(field: &str, name: &str) -> Result<(), PackageError> {
    if name.split('.').any(str::is_empty) {
        return Err(PackageError::new(format!(
            "{field} `{name}` must be a dotted Topaz module identity"
        )));
    }
    for (idx, segment) in name.split('.').enumerate() {
        validate_extern_identifier(&format!("{field} segment `{segment}`"), segment)?;
        if idx == 0 && matches!(segment, "std" | "topaz") {
            return Err(PackageError::new(format!(
                "{field} root `{segment}` is reserved for built-in modules"
            )));
        }
    }
    Ok(())
}

pub(crate) fn normalize_project_path(field: &str, raw: &str) -> Result<String, PackageError> {
    let raw = raw.replace('\\', "/");
    if raw.trim().is_empty() {
        return Err(PackageError::new(format!("{field} must be non-empty")));
    }
    if raw.starts_with('/') || Path::new(&raw).is_absolute() {
        return Err(PackageError::new(format!("{field} must be relative")));
    }
    if raw.split('/').any(|seg| seg == "..") {
        return Err(PackageError::new(format!(
            "{field} must not contain parent (`..`) segments"
        )));
    }
    let normalized = raw
        .split('/')
        .filter(|seg| !seg.is_empty() && *seg != ".")
        .collect::<Vec<_>>()
        .join("/");
    if normalized.is_empty() {
        return Err(PackageError::new(format!("{field} must name a path")));
    }
    Ok(normalized)
}

pub(crate) fn validate_local_dep_path(name: &str, raw: &str) -> Result<(), PackageError> {
    if raw.trim().is_empty() {
        return Err(PackageError::new(format!(
            "[dependencies].{name}.path must be non-empty"
        )));
    }
    if raw.replace('\\', "/").starts_with('/') || Path::new(raw).is_absolute() {
        return Err(PackageError::new(format!(
            "[dependencies].{name}.path must be relative"
        )));
    }
    Ok(())
}

pub(crate) fn validate_sha256_hash(field: &str, raw: &str) -> Result<(), PackageError> {
    let Some(hex) = raw.strip_prefix("sha256:") else {
        return Err(PackageError::new(format!(
            "{field} must start with `sha256:`"
        )));
    };
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(PackageError::new(format!(
            "{field} must be a 64-hex SHA-256 digest"
        )));
    }
    Ok(())
}

pub(crate) fn expect_table<'a>(
    value: &'a TomlValue,
    context: &str,
) -> Result<&'a BTreeMap<Rc<str>, TomlValue>, PackageError> {
    match value {
        TomlValue::Table(t) => Ok(t),
        _ => Err(PackageError::new(format!("{context} must be a TOML table"))),
    }
}

pub(crate) fn required_table<'a>(
    table: &'a BTreeMap<Rc<str>, TomlValue>,
    key: &str,
    context: &str,
) -> Result<&'a BTreeMap<Rc<str>, TomlValue>, PackageError> {
    optional_table(table, key)?
        .ok_or_else(|| PackageError::new(format!("{context}: missing `[{key}]`")))
}

pub(crate) fn optional_table<'a>(
    table: &'a BTreeMap<Rc<str>, TomlValue>,
    key: &str,
) -> Result<Option<&'a BTreeMap<Rc<str>, TomlValue>>, PackageError> {
    match table.get(key) {
        Some(TomlValue::Table(t)) => Ok(Some(t)),
        Some(_) => Err(PackageError::new(format!("`{key}` must be a table"))),
        None => Ok(None),
    }
}

pub(crate) fn required_string(
    table: &BTreeMap<Rc<str>, TomlValue>,
    key: &str,
    context: &str,
) -> Result<String, PackageError> {
    optional_string(table, key, context)?
        .ok_or_else(|| PackageError::new(format!("{context}: missing `{key}`")))
}

pub(crate) fn optional_string(
    table: &BTreeMap<Rc<str>, TomlValue>,
    key: &str,
    context: &str,
) -> Result<Option<String>, PackageError> {
    match table.get(key) {
        Some(TomlValue::String(s)) => Ok(Some(s.to_string())),
        Some(_) => Err(PackageError::new(format!(
            "{context}.{key} must be a string"
        ))),
        None => Ok(None),
    }
}

pub(crate) fn optional_bool(
    table: &BTreeMap<Rc<str>, TomlValue>,
    key: &str,
    context: &str,
) -> Result<Option<bool>, PackageError> {
    match table.get(key) {
        Some(TomlValue::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(PackageError::new(format!("{context}.{key} must be a bool"))),
        None => Ok(None),
    }
}

pub(crate) fn optional_u64(
    table: &BTreeMap<Rc<str>, TomlValue>,
    key: &str,
    context: &str,
) -> Result<Option<u64>, PackageError> {
    match table.get(key) {
        Some(TomlValue::Integer(value)) if *value > 0 => Ok(Some(*value as u64)),
        Some(TomlValue::Integer(_)) => Err(PackageError::new(format!(
            "{context}.{key} must be a positive integer"
        ))),
        Some(_) => Err(PackageError::new(format!(
            "{context}.{key} must be an integer"
        ))),
        None => Ok(None),
    }
}

pub(crate) fn optional_string_array(
    table: &BTreeMap<Rc<str>, TomlValue>,
    key: &str,
    context: &str,
) -> Result<Option<Vec<String>>, PackageError> {
    match table.get(key) {
        Some(TomlValue::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items.iter() {
                match item {
                    TomlValue::String(s) => out.push(s.to_string()),
                    _ => {
                        return Err(PackageError::new(format!(
                            "{context}.{key} must contain only strings"
                        )));
                    }
                }
            }
            Ok(Some(out))
        }
        Some(_) => Err(PackageError::new(format!(
            "{context}.{key} must be an array of strings"
        ))),
        None => Ok(None),
    }
}

pub(crate) fn required_string_array(
    table: &BTreeMap<Rc<str>, TomlValue>,
    key: &str,
    context: &str,
) -> Result<Vec<String>, PackageError> {
    optional_string_array(table, key, context)?
        .ok_or_else(|| PackageError::new(format!("{context}: missing `{key}`")))
}

pub(crate) fn reject_unknown_keys(
    table: &BTreeMap<Rc<str>, TomlValue>,
    context: &str,
    allowed: &[&str],
) -> Result<(), PackageError> {
    for key in table.keys() {
        if !allowed
            .iter()
            .any(|allowed_key| key.as_ref() == *allowed_key)
        {
            return Err(PackageError::new(format!("{context}: unknown key `{key}`")));
        }
    }
    Ok(())
}
