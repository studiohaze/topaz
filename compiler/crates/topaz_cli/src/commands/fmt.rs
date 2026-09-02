use crate::*;

pub(super) fn fmt_package(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    check_only: bool,
    compiler: CompilerSelection,
) -> ExitCode {
    let project = match topaz_package::Project::load(root.unwrap_or(".")) {
        Ok(project) => project,
        Err(e) => {
            eprintln!("topaz: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(selected) = version_arg
        && selected != project.manifest.package.language
    {
        eprintln!(
            "topaz: --language-version conflicts with topaz.toml [package].language \
             (manifest {}, CLI {})",
            lang_version_text(project.manifest.package.language),
            lang_version_text(selected)
        );
        return ExitCode::FAILURE;
    }
    let mut files = Vec::new();
    if let Err(e) = collect_fmt_files(&project.root, Path::new(""), &mut files) {
        eprintln!("topaz: {e}");
        return ExitCode::FAILURE;
    }
    fmt_files(
        &files,
        project.manifest.package.language,
        check_only,
        compiler,
    )
}

pub(super) fn fmt_entry(
    entry: &str,
    root: Option<&str>,
    version: LangVersion,
    check_only: bool,
    compiler: CompilerSelection,
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
    fmt_files(&[path], version, check_only, compiler)
}

pub(super) fn fmt_files(
    paths: &[PathBuf],
    version: LangVersion,
    check_only: bool,
    compiler: CompilerSelection,
) -> ExitCode {
    let mut updates = Vec::new();
    for path in paths {
        let src = match fs::read_to_string(path) {
            Ok(src) => src,
            Err(e) => {
                eprintln!("topaz: cannot read `{}`: {e}", path.to_string_lossy());
                return ExitCode::FAILURE;
            }
        };
        if !selected_parse_source_ok(path, &src, version, compiler) {
            return ExitCode::FAILURE;
        }
        let formatted = format_source_text(&src);
        if formatted != src {
            updates.push((path.clone(), formatted));
        }
    }
    if check_only {
        for (path, _) in &updates {
            eprintln!("topaz: formatting differs: {}", path.to_string_lossy());
        }
        eprintln!(
            "topaz: checked {} file(s), {} differs",
            paths.len(),
            updates.len()
        );
        return if updates.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    for (path, formatted) in &updates {
        if let Err(e) = fs::write(path, formatted) {
            eprintln!("topaz: cannot write `{}`: {e}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    }
    eprintln!(
        "topaz: formatted {} file(s), {} changed",
        paths.len(),
        updates.len()
    );
    ExitCode::SUCCESS
}

pub(super) fn selected_parse_source_ok(
    path: &Path,
    src: &str,
    version: LangVersion,
    compiler: CompilerSelection,
) -> bool {
    match compiler {
        CompilerSelection::Rust => parse_source_ok(path, src, version),
        CompilerSelection::SelfHosted => self_parse_source_ok(path, src, version),
    }
}

pub(super) fn self_parse_source_ok(path: &Path, src: &str, version: LangVersion) -> bool {
    if !version.uses_self_hosted_product_default() {
        eprintln!("topaz: self formatter requires a self-hosted-default language profile");
        return false;
    }
    let source_id = path.to_string_lossy();
    let preview = match topaz_self_frontend::preview_source(&source_id, src) {
        Ok(preview) => preview,
        Err(error) => {
            eprintln!(
                "topaz: self formatter parse gate stopped for `{}`: {error}",
                path.to_string_lossy()
            );
            eprintln!(
                "topaz: recovery: rerun the same command with `--compiler rust` (not executed)"
            );
            return false;
        }
    };
    if preview.diagnostics.is_empty() {
        trace_self_frontend_route("formatter-parse");
        return true;
    }
    let mut map = SourceMap::new();
    let file = match map.add_file(source_id.into_owned(), src.to_string()) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("topaz: self formatter source map is invalid: {error}");
            return false;
        }
    };
    for source in &preview.diagnostics {
        let lo = source.lo as usize;
        let hi = source.hi as usize;
        if lo > hi || hi > src.len() || !src.is_char_boundary(lo) || !src.is_char_boundary(hi) {
            eprintln!(
                "topaz: self formatter diagnostic carries invalid span {}..{}",
                source.lo, source.hi
            );
            continue;
        }
        let diagnostic = Diagnostic::error(
            Code::new(SELF_DIAGNOSTIC_CODE_PLACEHOLDER),
            source.message.clone(),
            Label::new(Span::new(file, source.lo, source.hi), ""),
        );
        eprintln!(
            "{}",
            render_self_diagnostic(&diagnostic, &source.code, &map, false)
        );
    }
    false
}

pub(super) fn parse_source_ok(path: &Path, src: &str, version: LangVersion) -> bool {
    let mut map = SourceMap::new();
    let file = match map.add_file(path.to_string_lossy().into_owned(), src.to_string()) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("topaz: {e}");
            return false;
        }
    };
    let out = parse_with_options(
        file,
        map.file(file).src(),
        ParseOptions {
            language_version: version,
        },
    );
    for diag in &out.diagnostics {
        eprintln!("{}", render(diag, &map));
    }
    !has_errors(&out.diagnostics)
}

pub(super) fn format_source_text(src: &str) -> String {
    if src.is_empty() {
        return String::new();
    }
    let mut lines: Vec<String> = src
        .lines()
        .map(|line| line.trim_end_matches([' ', '\t']).to_string())
        .collect();
    organize_import_lines(&mut lines);
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

pub(super) fn collect_fmt_files(
    root: &Path,
    rel: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let dir = root.join(rel);
    let mut entries = fs::read_dir(&dir)
        .map_err(|e| format!("cannot read package dir `{}`: {e}", dir.to_string_lossy()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("cannot read package dir `{}`: {e}", dir.to_string_lossy()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|e| format!("cannot inspect `{}`: {e}", entry.path().to_string_lossy()))?;
        let child_rel = rel.join(entry.file_name());
        if file_type.is_dir() {
            if ignored_fmt_dir(&entry.file_name()) {
                continue;
            }
            collect_fmt_files(root, &child_rel, out)?;
        } else if file_type.is_file() && entry.path().extension().is_some_and(|ext| ext == "tpz") {
            out.push(entry.path());
        }
    }
    Ok(())
}

pub(super) fn ignored_fmt_dir(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(".git" | ".topaz" | "target" | "node_modules" | "vendor")
    )
}
