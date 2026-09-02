use crate::*;

pub(super) fn add_package_dependency(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    spec: &str,
    path: Option<&str>,
) -> ExitCode {
    if version_arg.is_some() {
        eprintln!("topaz: `add` uses topaz.toml [package].language; drop --language-version");
        return ExitCode::FAILURE;
    }
    let root = root.unwrap_or(".");
    let project = match topaz_package::Project::load(root) {
        Ok(project) => project,
        Err(e) => {
            eprintln!("topaz: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (name, line) = match add_dependency_line(&project, spec, path) {
        Ok(dep) => dep,
        Err(e) => {
            eprintln!("topaz: {e}");
            return ExitCode::FAILURE;
        }
    };
    if project.manifest.dependencies.contains_key(&name) {
        eprintln!("topaz: dependency `{name}` already exists in topaz.toml");
        return ExitCode::FAILURE;
    }
    let updated = append_dependency_line(&project.manifest_text, &line);
    if let Err(e) = topaz_package::parse_manifest(&updated) {
        eprintln!("topaz: internal add rendered an invalid topaz.toml: {e}");
        return ExitCode::FAILURE;
    }
    let manifest_path = project.root.join("topaz.toml");
    if let Err(e) = fs::write(&manifest_path, updated) {
        eprintln!(
            "topaz: cannot write `{}`: {e}",
            manifest_path.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }
    eprintln!(
        "topaz: added dependency `{name}` to `{}`",
        manifest_path.to_string_lossy()
    );
    ExitCode::SUCCESS
}

pub(super) fn add_dependency_line(
    project: &topaz_package::Project,
    spec: &str,
    path: Option<&str>,
) -> Result<(String, String), String> {
    match path {
        Some(path) => {
            if spec.contains('@') {
                return Err("local dependency syntax is `topaz add <name> --path <path>`".into());
            }
            validate_add_dep_name(spec)?;
            if spec == "std" {
                return Err("`std` is managed by the language version, not `topaz add`".into());
            }
            validate_add_dep_path(path)?;
            let dep_root = project.root.join(path);
            let dep_project = topaz_package::Project::load(&dep_root).map_err(|e| {
                format!("cannot load local dependency `{}`: {e}", dep_root.display())
            })?;
            if dep_project.manifest.package.name != spec {
                return Err(format!(
                    "local dependency `{spec}` points to `{}` whose [package].name is `{}`",
                    dep_root.to_string_lossy(),
                    dep_project.manifest.package.name
                ));
            }
            let hash = topaz_package::package_content_hash(&dep_root).map_err(|e| e.to_string())?;
            let mut line = String::new();
            line.push_str(spec);
            line.push_str(" = { path = ");
            push_toml_basic_string(&mut line, path);
            line.push_str(", hash = ");
            push_toml_basic_string(&mut line, &hash);
            line.push_str(" }");
            Ok((spec.to_string(), line))
        }
        None => {
            let Some((name, version)) = spec.split_once('@') else {
                return Err("registry dependency syntax is `topaz add <name>@<version>`".into());
            };
            validate_add_dep_name(name)?;
            validate_add_dep_version(version)?;
            if name == "std" {
                return Err("`std` is managed by the language version, not `topaz add`".into());
            }
            let mut line = String::new();
            line.push_str(name);
            line.push_str(" = ");
            push_toml_basic_string(&mut line, version);
            Ok((name.to_string(), line))
        }
    }
}

pub(super) fn append_dependency_line(manifest_text: &str, line: &str) -> String {
    let mut lines: Vec<String> = manifest_text.lines().map(str::to_string).collect();
    if let Some(dep_idx) = lines
        .iter()
        .position(|line| line.trim() == "[dependencies]")
    {
        let insert_idx = lines
            .iter()
            .enumerate()
            .skip(dep_idx + 1)
            .find_map(|(idx, line)| {
                let trimmed = line.trim();
                (trimmed.starts_with('[') && trimmed.ends_with(']')).then_some(idx)
            })
            .unwrap_or(lines.len());
        lines.insert(insert_idx, line.to_string());
    } else {
        if lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push("[dependencies]".to_string());
        lines.push(line.to_string());
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

pub(super) fn validate_add_dep_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "dependency name `{name}` must contain only ASCII letters, digits, `_`, or `-`"
        ));
    }
    Ok(())
}

pub(super) fn validate_add_dep_version(version: &str) -> Result<(), String> {
    if version.is_empty()
        || !version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
    {
        return Err(format!(
            "dependency version `{version}` must contain only ASCII letters, digits, `.`, `_`, `-`, or `+`"
        ));
    }
    Ok(())
}

pub(super) fn validate_add_dep_path(path: &str) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("local dependency path must be non-empty".into());
    }
    if path.replace('\\', "/").starts_with('/') || Path::new(path).is_absolute() {
        return Err("local dependency path must be relative".into());
    }
    Ok(())
}

pub(super) fn push_toml_basic_string(out: &mut String, raw: &str) {
    out.push('"');
    for ch in raw.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
