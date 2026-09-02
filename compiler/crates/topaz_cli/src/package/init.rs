use crate::*;

pub(super) fn init_package(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    target_arg: Option<&str>,
) -> ExitCode {
    if let Some(version) = version_arg
        && version != LangVersion::CURRENT
    {
        eprintln!(
            "topaz: `init` scaffolds v{} packages; drop --language-version or use {}",
            LangVersion::CURRENT.as_str(),
            LangVersion::CURRENT.as_str()
        );
        return ExitCode::FAILURE;
    }
    let (web_app, http_service) = match target_arg {
        None | Some("native") => (false, false),
        Some("web-app") => (true, false),
        Some("http-service") => (false, true),
        Some(other) => {
            eprintln!(
                "topaz: `init --target {other}` is unsupported (expected `native`, `web-app`, or `http-service`)"
            );
            return ExitCode::FAILURE;
        }
    };
    let root = PathBuf::from(root.unwrap_or("."));
    if let Err(e) = fs::create_dir_all(&root) {
        eprintln!(
            "topaz: cannot create package root `{}`: {e}",
            root.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }
    let root = match fs::canonicalize(&root) {
        Ok(root) => root,
        Err(e) => {
            eprintln!(
                "topaz: cannot resolve package root `{}`: {e}",
                root.to_string_lossy()
            );
            return ExitCode::FAILURE;
        }
    };
    let manifest = root.join("topaz.toml");
    let entry = root.join("src/main.tpz");
    let style = root.join("styles/app.css");
    let test = root.join("tests/app.tpz");
    for path in [&manifest, &entry]
        .into_iter()
        .chain(web_app.then_some(&style))
        .chain(web_app.then_some(&test))
    {
        if path.exists() {
            eprintln!(
                "topaz: refusing to overwrite existing `{}`",
                path.to_string_lossy()
            );
            return ExitCode::FAILURE;
        }
    }
    if let Err(e) = fs::create_dir_all(root.join("src")) {
        eprintln!(
            "topaz: cannot create `{}`: {e}",
            root.join("src").to_string_lossy()
        );
        return ExitCode::FAILURE;
    }
    let name = scaffold_package_name(&root);
    let lang = lang_version_text(LangVersion::CURRENT);
    let manifest_text = if web_app {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nlanguage = \"{lang}\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"web-app\"\ndeterministic = true\n\n[web]\ntitle = \"Topaz application\"\nstyles = [\"styles/app.css\"]\nassets = []\nlifecycle = \"v2\"\n\n[dependencies]\nstd = \"{lang}\"\n"
        )
    } else if http_service {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nlanguage = \"{lang}\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"http-service\"\ndeterministic = true\n\n[service]\nbind = \"127.0.0.1\"\nport = 8080\nworkers = 1\nmax_connections = 64\nqueue_capacity = 32\nmax_target_bytes = 8192\nmax_header_bytes = 16384\nmax_headers = 64\nmax_body_bytes = 1048576\nheader_timeout_ms = 5000\nbody_timeout_ms = 5000\nhandler_timeout_ms = 1000\nshutdown_grace_ms = 5000\nlog_format = \"text\"\n\n[dependencies]\nstd = \"{lang}\"\n"
        )
    } else {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nlanguage = \"{lang}\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"native\"\ndeterministic = true\n\n[dependencies]\nstd = \"{lang}\"\n"
        )
    };
    let entry_text = if web_app {
        "import std.dom { Html, WebAppEvent, WebAppStep, text }\n\nexport record Model {\n  message: string,\n}\n\nexport enum Msg {\n  Ready,\n}\n\nexport function init() -> WebAppStep<Model, Msg> {\n  WebAppStep { model: Model { message: \"Hello from Topaz\" }, commands: [] }\n}\n\nexport function update(model: Model, message: Msg, event: WebAppEvent) -> WebAppStep<Model, Msg> {\n  WebAppStep { model: model, commands: [] }\n}\n\nexport function view(model: Model) -> Html<Msg> {\n  text(model.message)\n}\n"
    } else if http_service {
        "import std.http { HttpRequest, HttpResponse, text }\n\nexport function handle(req: HttpRequest) -> HttpResponse {\n  if req.url.path() == \"/health\" {\n    return text(200, \"ok\")\n  }\n  text(404, \"not found\")\n}\n"
    } else {
        "export function main(args: Array<string>, stdin: string) -> Result<int, string> {\n    print(\"Hello from Topaz\")\n    Ok(0)\n}\n"
    };
    if let Err(e) = write_new_text_file(&manifest, &manifest_text) {
        eprintln!("topaz: cannot write `{}`: {e}", manifest.to_string_lossy());
        return ExitCode::FAILURE;
    }
    if let Err(e) = write_new_text_file(&entry, entry_text) {
        eprintln!("topaz: cannot write `{}`: {e}", entry.to_string_lossy());
        return ExitCode::FAILURE;
    }
    if web_app {
        if let Err(e) = fs::create_dir_all(root.join("styles"))
            .and_then(|()| fs::create_dir_all(root.join("tests")))
        {
            eprintln!("topaz: cannot create web-app directories: {e}");
            return ExitCode::FAILURE;
        }
        if let Err(e) = write_new_text_file(
            &style,
            "html { font-family: system-ui, sans-serif; }\nbody { margin: 2rem; }\n",
        ) {
            eprintln!("topaz: cannot write `{}`: {e}", style.to_string_lossy());
            return ExitCode::FAILURE;
        }
        if let Err(e) = write_new_text_file(&test, "print(\"web-app scaffold test\")\n") {
            eprintln!("topaz: cannot write `{}`: {e}", test.to_string_lossy());
            return ExitCode::FAILURE;
        }
    }
    eprintln!(
        "topaz: initialized package `{name}` at `{}`",
        root.to_string_lossy()
    );
    ExitCode::SUCCESS
}

pub(super) fn write_new_text_file(path: &Path, contents: &str) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(contents.as_bytes())
}

pub(super) fn scaffold_package_name(root: &Path) -> String {
    let raw = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("topaz_app");
    let mut name = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            name.push(ch);
        } else {
            name.push('_');
        }
    }
    if name.is_empty() {
        "topaz_app".to_string()
    } else {
        name
    }
}

pub(super) fn migrate_package_or_entry(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    from: Option<&str>,
    to: Option<&str>,
    entry: Option<&str>,
) -> ExitCode {
    if version_arg.is_some() {
        eprintln!("topaz: `migrate` uses `--from`/`--to`; drop --language-version");
        return ExitCode::FAILURE;
    }
    let Some(from) = migrate_version_arg("--from", from) else {
        return ExitCode::FAILURE;
    };
    let Some(to) = migrate_version_arg("--to", to) else {
        return ExitCode::FAILURE;
    };
    if !matches!(
        (from, to),
        (LangVersion::V5_3, LangVersion::V5_4)
            | (LangVersion::V5_6, LangVersion::V5_7)
            | (LangVersion::V5_7, LangVersion::V5_8)
    ) {
        eprintln!(
            "topaz: `migrate` supports only adjacent adopted boundaries: 5.3 to 5.4, 5.6 to 5.7, or 5.7 to 5.8"
        );
        return ExitCode::FAILURE;
    }
    match entry {
        Some(entry) => migrate_entry_file(root, entry, from, to),
        None if (from, to) == (LangVersion::V5_6, LangVersion::V5_7) => {
            migrate_package_56_to_57(root, LangVersion::CURRENT)
        }
        None if (from, to) == (LangVersion::V5_7, LangVersion::V5_8) => {
            migrate_package_language_boundary(root, from, to, LangVersion::CURRENT)
        }
        None => migrate_package(root, from, to),
    }
}

pub(super) fn migrate_version_arg(flag: &str, value: Option<&str>) -> Option<LangVersion> {
    let Some(value) = value else {
        eprintln!("topaz: `migrate` requires `{flag} <version>`");
        return None;
    };
    match LangVersion::parse_exact(value) {
        Some(version) if version.is_selectable() => Some(version),
        Some(_) => {
            eprintln!(
                "topaz: `{flag}` version `{value}` is known but not current in this toolchain"
            );
            None
        }
        None => {
            eprintln!(
                "topaz: unknown `{flag}` version `{value}` (expected a current language line from 5.1 through 5.10)"
            );
            None
        }
    }
}

pub(super) fn migrate_entry_file(
    root: Option<&str>,
    entry: &str,
    from: LangVersion,
    to: LangVersion,
) -> ExitCode {
    let entry = entry.replace('\\', "/");
    let (base, entry_rel, _) = match split_absolute(&entry, root) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("topaz: {msg}");
            return ExitCode::FAILURE;
        }
    };
    let path = Path::new(&base).join(entry_rel);
    let src = match fs::read_to_string(&path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("topaz: cannot read `{}`: {e}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };
    if !parse_source_ok(&path, &src, from) {
        eprintln!(
            "topaz: `{}` does not parse as {}",
            path.to_string_lossy(),
            lang_version_text(from)
        );
        return ExitCode::FAILURE;
    }
    if !parse_source_ok(&path, &src, to) {
        eprintln!(
            "topaz: `{}` does not parse as {}",
            path.to_string_lossy(),
            lang_version_text(to)
        );
        return ExitCode::FAILURE;
    }
    eprintln!(
        "topaz: migration checked `{}` ({} -> {}); no source rewrite needed",
        path.to_string_lossy(),
        lang_version_text(from),
        lang_version_text(to)
    );
    ExitCode::SUCCESS
}

pub(super) fn migrate_package(root: Option<&str>, from: LangVersion, to: LangVersion) -> ExitCode {
    let project = match topaz_package::Project::load(root.unwrap_or(".")) {
        Ok(project) => project,
        Err(e) => {
            eprintln!("topaz: {e}");
            return ExitCode::FAILURE;
        }
    };
    if project.manifest.package.language != from {
        eprintln!(
            "topaz: package language is {}, not requested --from {}",
            lang_version_text(project.manifest.package.language),
            lang_version_text(from)
        );
        return ExitCode::FAILURE;
    }
    let entry_path = project.root.join(&project.manifest.package.entry);
    let src = match fs::read_to_string(&entry_path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("topaz: cannot read `{}`: {e}", entry_path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };
    if !parse_source_ok(&entry_path, &src, from) {
        eprintln!(
            "topaz: package entry `{}` does not parse as {}",
            entry_path.to_string_lossy(),
            lang_version_text(from)
        );
        return ExitCode::FAILURE;
    }
    if !parse_source_ok(&entry_path, &src, to) {
        eprintln!(
            "topaz: package entry `{}` does not parse as {}",
            entry_path.to_string_lossy(),
            lang_version_text(to)
        );
        return ExitCode::FAILURE;
    }
    let (updated, changed) = migrate_manifest_53_to_54(&project.manifest_text);
    if changed == 0 {
        eprintln!("topaz: could not rewrite topaz.toml language metadata for migration");
        return ExitCode::FAILURE;
    }
    let manifest = match topaz_package::parse_manifest(&updated) {
        Ok(manifest) => manifest,
        Err(e) => {
            eprintln!("topaz: internal migrate rendered an invalid topaz.toml: {e}");
            return ExitCode::FAILURE;
        }
    };
    if manifest.package.language != to {
        eprintln!("topaz: internal migrate did not update [package].language");
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
        "topaz: migrated package `{}` from {} to {}; changed topaz.toml",
        project.manifest.package.name,
        lang_version_text(from),
        lang_version_text(to)
    );
    ExitCode::SUCCESS
}

pub(super) fn migrate_package_56_to_57(root: Option<&str>, current: LangVersion) -> ExitCode {
    migrate_package_language_boundary(root, LangVersion::V5_6, LangVersion::V5_7, current)
}

pub(super) fn migrate_package_language_boundary(
    root: Option<&str>,
    from: LangVersion,
    to: LangVersion,
    current: LangVersion,
) -> ExitCode {
    let root = root.unwrap_or(".");
    let project = match topaz_package::Project::load(root) {
        Ok(project) => project,
        Err(error) => {
            eprintln!("topaz: {error}");
            return ExitCode::FAILURE;
        }
    };
    if project.manifest.package.language != from {
        eprintln!(
            "topaz: package language is {}, not requested --from {}",
            lang_version_text(project.manifest.package.language),
            from.as_str()
        );
        return ExitCode::FAILURE;
    }

    let mut target = match package_target(Some(root), None, false) {
        Ok(target) => target,
        Err(code) => return code,
    };
    for version in [from, to] {
        target.version = version;
        let resolved = resolve_package_target(&target);
        for diagnostic in &resolved.diagnostics {
            eprintln!("{}", render(diagnostic, &resolved.map));
        }
        if has_errors(&resolved.diagnostics) {
            eprintln!(
                "topaz: package does not resolve as topaz-{}; no files changed",
                version.as_str()
            );
            return ExitCode::FAILURE;
        }
        if let Err(count) = check_resolved_unit(&resolved, false, version) {
            eprintln!(
                "topaz: package does not check as topaz-{} ({count} diagnostic{}); no files changed",
                version.as_str(),
                if count == 1 { "" } else { "s" }
            );
            return ExitCode::FAILURE;
        }
    }

    let (updated, changed) =
        migrate_manifest_language(&project.manifest_text, from.as_str(), to.as_str());
    let language_marker = format!("language = \"{}\"", to.as_str());
    let std_marker = format!("std = \"{}\"", to.as_str());
    if changed == 0 || !updated.contains(&language_marker) || !updated.contains(&std_marker) {
        eprintln!(
            "topaz: could not render exact {} to {} package metadata migration; no files changed",
            from.as_str(),
            to.as_str()
        );
        return ExitCode::FAILURE;
    }
    if to > current {
        eprintln!(
            "topaz: package is compatible with topaz-{}, but {} is not current in this toolchain; no files changed",
            to.as_str(),
            to.as_str()
        );
        return ExitCode::FAILURE;
    }

    let manifest_path = project.root.join("topaz.toml");
    if let Err(error) = write_atomic_text(&manifest_path, &updated) {
        eprintln!(
            "topaz: cannot atomically update `{}`: {error}; no source, lock, or vendor file changed",
            manifest_path.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }
    eprintln!(
        "topaz: migrated package `{}` from {} to {}; changed topaz.toml only",
        project.manifest.package.name,
        from.as_str(),
        to.as_str()
    );
    ExitCode::SUCCESS
}

pub(super) fn write_atomic_text(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("topaz.toml");
    let temporary =
        path.with_file_name(format!(".{file_name}.topaz-migrate-{}", std::process::id()));
    let backup = path.with_file_name(format!(
        ".{file_name}.topaz-migrate-backup-{}",
        std::process::id()
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        if backup.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("stale migration backup `{}`", backup.display()),
            ));
        }
        fs::rename(path, &backup)?;
        if let Err(error) = fs::rename(&temporary, path) {
            let _ = fs::rename(&backup, path);
            return Err(error);
        }
        let _ = fs::remove_file(&backup);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
        if backup.exists() && !path.exists() {
            let _ = fs::rename(&backup, path);
        }
    }
    result
}

pub(super) fn migrate_manifest_53_to_54(manifest_text: &str) -> (String, usize) {
    migrate_manifest_language(manifest_text, "5.3", "5.4")
}

pub(super) fn migrate_manifest_language(
    manifest_text: &str,
    from: &str,
    to: &str,
) -> (String, usize) {
    let trailing_newline = manifest_text.ends_with('\n');
    let mut section = String::new();
    let mut changed = 0usize;
    let lines: Vec<String> = manifest_text
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                section = trimmed.to_string();
            }
            if section == "[package]"
                && let Some(updated) = rewrite_toml_string_value(line, "language", from, to)
            {
                changed += 1;
                return updated;
            }
            if section == "[dependencies]"
                && let Some(updated) = rewrite_toml_string_value(line, "std", from, to)
            {
                changed += 1;
                return updated;
            }
            line.to_string()
        })
        .collect();
    if lines.is_empty() {
        return (String::new(), changed);
    }
    let mut out = lines.join("\n");
    if trailing_newline {
        out.push('\n');
    }
    (out, changed)
}

pub(super) fn rewrite_toml_string_value(
    line: &str,
    key: &str,
    from: &str,
    to: &str,
) -> Option<String> {
    let trimmed_start = line.trim_start();
    let after_key = trimmed_start.strip_prefix(key)?;
    if !after_key.trim_start().starts_with('=') {
        return None;
    }
    let after_eq = after_key.trim_start().strip_prefix('=')?;
    let from_lit = format!("\"{from}\"");
    if !after_eq.trim_start().starts_with(&from_lit) {
        return None;
    }
    Some(line.replacen(&from_lit, &format!("\"{to}\""), 1))
}
