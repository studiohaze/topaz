use crate::*;

/// Parses one file; renders every diagnostic; optionally dumps the
/// AST. Exit status is the parse-ok verdict.
pub(super) fn parse_file(
    path: &str,
    dump_ast: bool,
    version: LangVersion,
    compiler_selection: CompilerSelection,
) -> ExitCode {
    if compiler_selection == CompilerSelection::SelfHosted {
        let normalized = path.replace('\\', "/");
        let (base, entry, root) = match split_absolute(&normalized, None) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("topaz: {error}");
                return ExitCode::FAILURE;
            }
        };
        let request = topaz_kernel::KernelRequest::checked(
            &entry,
            root.as_deref(),
            version,
            topaz_kernel::PackageFacts::standalone(),
        );
        let product = match compile_self_product(
            &PhysicalFactHost::new(base),
            request,
            None,
            &format!(
                "rerun `topaz {} {path} --compiler rust`",
                if dump_ast { "dump-ast" } else { "parse" }
            ),
        ) {
            Ok(product) => product,
            Err(code) => return code,
        };
        let Some(module) = product
            .typed()
            .resolved
            .modules
            .iter()
            .find(|module| module.entry)
        else {
            eprintln!("topaz: self compilation product omitted the entry module");
            return ExitCode::FAILURE;
        };
        if dump_ast {
            println!("{:#?}", module.ast);
        }
        let map = match self_preview_source_map(&product.typed().resolved.modules) {
            Ok(map) => map,
            Err(error) => {
                eprintln!("topaz: self compilation product is invalid: {error}");
                return ExitCode::FAILURE;
            }
        };
        for diagnostic in &product.typed().resolved.diagnostics {
            match self_resolver_diagnostic(diagnostic, &product.typed().resolved.modules) {
                Ok(rendered) => eprintln!(
                    "{}",
                    render_self_diagnostic(&rendered, &diagnostic.code, &map, false)
                ),
                Err(error) => {
                    eprintln!("topaz: self compilation product is invalid: {error}");
                    return ExitCode::FAILURE;
                }
            }
        }
        if product.typed().resolved.diagnostics.is_empty() {
            if !dump_ast {
                println!("{path}: parse-ok");
            }
            return ExitCode::SUCCESS;
        }
        eprintln!(
            "{path}: {} diagnostic{}",
            product.typed().resolved.diagnostics.len(),
            if product.typed().resolved.diagnostics.len() == 1 {
                ""
            } else {
                "s"
            }
        );
        return ExitCode::FAILURE;
    }
    // Loader boundary (CDR-001 §5): an oversized file is a loader
    // error reported here, not a span-bearing diagnostic — checked
    // before reading so the rejection never costs the allocation.
    match fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_SOURCE_LEN as u64 => {
            let err = SourceMapError::FileTooLarge {
                name: path.to_string(),
                len: usize::try_from(meta.len()).unwrap_or(usize::MAX),
            };
            eprintln!("topaz: {err}");
            return ExitCode::FAILURE;
        }
        Ok(_) => {}
        Err(e) => {
            eprintln!("topaz: {path}: {e}");
            return ExitCode::FAILURE;
        }
    }
    let src = match fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("topaz: {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut map = SourceMap::new();
    let file = match map.add_file(path, src) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("topaz: {e}");
            return ExitCode::FAILURE;
        }
    };
    let out = parse_with_options(
        file,
        map.file(file).src(),
        ParseOptions {
            language_version: version,
        },
    );
    if dump_ast {
        println!("{:#?}", out.program);
    }
    for diag in &out.diagnostics {
        eprintln!("{}", render(diag, &map));
    }
    if !has_errors(&out.diagnostics) {
        if !dump_ast {
            println!("{path}: parse-ok");
        }
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "{path}: {} diagnostic{}",
            out.diagnostics.len(),
            if out.diagnostics.len() == 1 { "" } else { "s" }
        );
        ExitCode::FAILURE
    }
}

pub(super) fn refactor_rename(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    old_name: &str,
    new_name: &str,
    entry: Option<&str>,
    default_version: LangVersion,
) -> ExitCode {
    if old_name == new_name {
        eprintln!("topaz: refactor rename source and target are identical");
        return ExitCode::FAILURE;
    }
    if !lsp_rename_name_is_valid(old_name) {
        eprintln!("topaz: refactor rename source `{old_name}` is not a bindable identifier");
        return ExitCode::FAILURE;
    }
    if !lsp_rename_name_is_valid(new_name) {
        eprintln!("topaz: refactor rename target `{new_name}` is not a bindable identifier");
        return ExitCode::FAILURE;
    }

    let (path, version) = match refactor_entry_path(root, version_arg, entry, default_version) {
        Ok(target) => target,
        Err(code) => return code,
    };
    let src = match fs::read_to_string(&path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("topaz: cannot read `{}`: {e}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };
    if !parse_source_ok(&path, &src, version) {
        return ExitCode::FAILURE;
    }

    let mut definitions = Vec::new();
    let candidates = lsp_identifier_candidate_spans(&src);
    for span in &candidates {
        if source_span_text(&src, *span) != Some(old_name) {
            continue;
        }
        let Some(definition) = lsp_definition_at(&src, span.lo, version) else {
            continue;
        };
        if source_span_text(&src, definition) == Some(old_name) {
            definitions.push(definition);
        }
    }
    definitions.sort_by_key(|span| (span.lo, span.hi));
    definitions.dedup_by_key(|span| (span.lo, span.hi));
    let [definition] = definitions.as_slice() else {
        if definitions.is_empty() {
            eprintln!(
                "topaz: refactor rename could not find a lexical binding named `{old_name}` in `{}`",
                path.to_string_lossy()
            );
        } else {
            eprintln!(
                "topaz: refactor rename found {} lexical bindings named `{old_name}` in `{}`; \
                 choose a position-sensitive LSP rename for this file",
                definitions.len(),
                path.to_string_lossy()
            );
        }
        return ExitCode::FAILURE;
    };

    let mut replacements: Vec<_> = candidates
        .into_iter()
        .filter(|span| lsp_definition_at(&src, span.lo, version) == Some(*definition))
        .collect();
    replacements.sort_by_key(|span| (span.lo, span.hi));
    replacements.dedup_by_key(|span| (span.lo, span.hi));
    if replacements.is_empty() {
        eprintln!(
            "topaz: refactor rename found no references for `{old_name}` in `{}`",
            path.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }

    let updated = replace_source_spans(&src, &replacements, new_name);
    if !parse_source_ok(&path, &updated, version) {
        eprintln!(
            "topaz: refactor rename would make `{}` fail to parse; no files changed",
            path.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }
    if let Err(e) = fs::write(&path, updated) {
        eprintln!("topaz: cannot write `{}`: {e}", path.to_string_lossy());
        return ExitCode::FAILURE;
    }
    eprintln!(
        "topaz: renamed `{old_name}` to `{new_name}` in {} occurrence(s) in `{}`",
        replacements.len(),
        path.to_string_lossy()
    );
    ExitCode::SUCCESS
}

pub(super) fn refactor_organize_imports(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    entry: Option<&str>,
    default_version: LangVersion,
) -> ExitCode {
    let (path, version) = match refactor_entry_path(root, version_arg, entry, default_version) {
        Ok(target) => target,
        Err(code) => return code,
    };
    let src = match fs::read_to_string(&path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("topaz: cannot read `{}`: {e}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };
    if !parse_source_ok(&path, &src, version) {
        return ExitCode::FAILURE;
    }
    let organized = organize_imports_source_text(&src);
    if organized == src {
        eprintln!("topaz: organized imports in 1 file(s), 0 changed");
        return ExitCode::SUCCESS;
    }
    if !parse_source_ok(&path, &organized, version) {
        eprintln!(
            "topaz: refactor organize-imports would make `{}` fail to parse; no files changed",
            path.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }
    if let Err(e) = fs::write(&path, organized) {
        eprintln!("topaz: cannot write `{}`: {e}", path.to_string_lossy());
        return ExitCode::FAILURE;
    }
    eprintln!("topaz: organized imports in 1 file(s), 1 changed");
    ExitCode::SUCCESS
}

pub(super) fn refactor_add_missing_match_cases(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    entry: Option<&str>,
    default_version: LangVersion,
) -> ExitCode {
    let (path, version) = match refactor_entry_path(root, version_arg, entry, default_version) {
        Ok(target) => target,
        Err(code) => return code,
    };
    let src = match fs::read_to_string(&path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("topaz: cannot read `{}`: {e}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };
    let Some(program) = parse_refactor_program(&path, &src, version) else {
        return ExitCode::FAILURE;
    };
    let (_, diagnostics) = lsp_diagnostics(&src, version);
    let non_exhaustive = topaz_check::codes::NON_EXHAUSTIVE;
    if diagnostics.is_empty() {
        eprintln!("topaz: added missing match cases in 1 file(s), 0 changed");
        return ExitCode::SUCCESS;
    }
    if let Some(other) = diagnostics.iter().find(|diag| diag.code != non_exhaustive) {
        eprintln!(
            "topaz: refactor add-missing-match-cases requires only TPZ5021 diagnostics; \
             found {}",
            other.code
        );
        return ExitCode::FAILURE;
    }

    let edits = match missing_match_case_edits(&src, &program, &diagnostics) {
        Ok(edits) => edits,
        Err(msg) => {
            eprintln!("topaz: {msg}");
            return ExitCode::FAILURE;
        }
    };
    if edits.is_empty() {
        eprintln!("topaz: added missing match cases in 1 file(s), 0 changed");
        return ExitCode::SUCCESS;
    }
    let updated = insert_source_text(&src, &edits);
    if !parse_source_ok(&path, &updated, version) {
        eprintln!(
            "topaz: refactor add-missing-match-cases would make `{}` fail to parse; no files changed",
            path.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }
    let (updated_map, updated_diagnostics) = lsp_diagnostics(&updated, version);
    if has_errors(&updated_diagnostics) {
        for diag in &updated_diagnostics {
            eprintln!("{}", render(diag, &updated_map));
        }
        eprintln!(
            "topaz: generated match cases would not type-check in `{}`; no files changed",
            path.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }
    if let Err(e) = fs::write(&path, updated) {
        eprintln!("topaz: cannot write `{}`: {e}", path.to_string_lossy());
        return ExitCode::FAILURE;
    }
    eprintln!(
        "topaz: added missing match cases in 1 file(s), {} changed",
        edits.len()
    );
    ExitCode::SUCCESS
}

pub(super) fn refactor_derive_json(
    root: Option<&str>,
    location: &str,
    version: LangVersion,
) -> ExitCode {
    let (path, line) = match parse_file_line_location(root, location) {
        Ok(location) => location,
        Err(msg) => {
            eprintln!("topaz: {msg}");
            return ExitCode::FAILURE;
        }
    };
    let src = match fs::read_to_string(&path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("topaz: cannot read `{}`: {e}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };
    let Some(program) = parse_refactor_program(&path, &src, version) else {
        return ExitCode::FAILURE;
    };
    let edit = match derive_json_edit(&src, &program, line) {
        Ok(Some(edit)) => edit,
        Ok(None) => {
            eprintln!("topaz: added JSON derive in 1 file(s), 0 changed");
            return ExitCode::SUCCESS;
        }
        Err(msg) => {
            eprintln!("topaz: {msg}");
            return ExitCode::FAILURE;
        }
    };
    let updated = insert_source_text(&src, &[edit]);
    if !parse_source_ok(&path, &updated, version) {
        eprintln!(
            "topaz: refactor derive-json would make `{}` fail to parse; no files changed",
            path.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }
    let (updated_map, updated_diagnostics) = lsp_diagnostics(&updated, version);
    if has_errors(&updated_diagnostics) {
        for diag in &updated_diagnostics {
            eprintln!("{}", render(diag, &updated_map));
        }
        eprintln!(
            "topaz: generated JSON derive would not type-check in `{}`; no files changed",
            path.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }
    if let Err(e) = fs::write(&path, updated) {
        eprintln!("topaz: cannot write `{}`: {e}", path.to_string_lossy());
        return ExitCode::FAILURE;
    }
    eprintln!("topaz: added JSON derive in 1 file(s), 1 changed");
    ExitCode::SUCCESS
}

#[derive(Debug, Clone)]
pub(super) struct SourceInsert {
    pub(super) at: usize,
    pub(super) text: String,
}

#[derive(Debug, Clone)]
pub(super) struct DeriveJsonTarget {
    pub(super) span: Span,
    pub(super) name: Span,
    pub(super) derives: Vec<Span>,
}

#[derive(Debug, Clone)]
pub(super) struct MissingEnumVariant {
    pub(super) enum_name: String,
    pub(super) variant_name: String,
}

pub(super) fn parse_file_line_location(
    root: Option<&str>,
    location: &str,
) -> Result<(PathBuf, u32), String> {
    let (path, line) = location
        .rsplit_once(':')
        .ok_or_else(|| "`refactor derive-json` needs <file>:<line>".to_string())?;
    if path.is_empty() {
        return Err("derive-json path is empty".to_string());
    }
    let line: u32 = line
        .parse()
        .map_err(|_| format!("derive-json line `{line}` is not a positive integer"))?;
    if line == 0 {
        return Err("derive-json line must be >= 1".to_string());
    }
    let path = PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        PathBuf::from(root.unwrap_or(".")).join(path)
    };
    Ok((path, line))
}

pub(super) fn derive_json_edit(
    src: &str,
    program: &ast::Program,
    line: u32,
) -> Result<Option<SourceInsert>, String> {
    let Some((line_start, line_end)) = source_line_bounds(src, line) else {
        return Err(format!("line {line} is outside the source file"));
    };
    let mut targets = Vec::new();
    for stmt in &program.items {
        collect_derive_json_targets(stmt, line_start, line_end, &mut targets);
    }
    let [target] = targets.as_slice() else {
        return match targets.len() {
            0 => Err(format!(
                "line {line} does not select a record or enum declaration"
            )),
            n => Err(format!(
                "line {line} selects {n} record/enum declarations; choose a more specific line"
            )),
        };
    };
    if target
        .derives
        .iter()
        .any(|span| source_span_text(src, *span) == Some("JSON"))
    {
        return Ok(None);
    }
    if let Some(last) = target.derives.iter().max_by_key(|span| span.hi) {
        return Ok(Some(SourceInsert {
            at: last.hi as usize,
            text: ", JSON".to_string(),
        }));
    }
    let open = nominal_body_open(src, target)?;
    let needs_leading_space = src[..open]
        .chars()
        .last()
        .is_some_and(|ch| !ch.is_whitespace());
    Ok(Some(SourceInsert {
        at: open,
        text: if needs_leading_space {
            " derives JSON ".to_string()
        } else {
            "derives JSON ".to_string()
        },
    }))
}

pub(super) fn collect_derive_json_targets(
    stmt: &ast::Stmt,
    line_start: usize,
    line_end: usize,
    out: &mut Vec<DeriveJsonTarget>,
) {
    match &stmt.kind {
        ast::StmtKind::Export(inner) => {
            collect_derive_json_targets(inner, line_start, line_end, out)
        }
        ast::StmtKind::Record(decl)
            if span_intersects_line(decl.name.span, line_start, line_end) =>
        {
            out.push(DeriveJsonTarget {
                span: stmt.span,
                name: decl.name.span,
                derives: decl.derives.iter().map(|ident| ident.span).collect(),
            });
        }
        ast::StmtKind::Enum(decl) if span_intersects_line(decl.name.span, line_start, line_end) => {
            out.push(DeriveJsonTarget {
                span: stmt.span,
                name: decl.name.span,
                derives: decl.derives.iter().map(|ident| ident.span).collect(),
            });
        }
        _ => {}
    }
}

pub(super) fn span_intersects_line(span: Span, line_start: usize, line_end: usize) -> bool {
    let start = span.lo as usize;
    let end = span.hi as usize;
    start <= line_end && end >= line_start
}

pub(super) fn source_line_bounds(src: &str, line: u32) -> Option<(usize, usize)> {
    let mut start = 0usize;
    for _ in 1..line {
        start += src.get(start..)?.find('\n')? + 1;
    }
    let end = src
        .get(start..)?
        .find('\n')
        .map_or(src.len(), |offset| start + offset);
    Some((start, end))
}

pub(super) fn nominal_body_open(src: &str, target: &DeriveJsonTarget) -> Result<usize, String> {
    let start = target.name.hi as usize;
    let end = target.span.hi as usize;
    let head = src
        .get(start..end)
        .ok_or_else(|| "record/enum declaration span is outside the source text".to_string())?;
    let rel = head
        .find('{')
        .ok_or_else(|| "record/enum declaration has no body opening brace".to_string())?;
    Ok(start + rel)
}

pub(super) fn parse_refactor_program(
    path: &Path,
    src: &str,
    version: LangVersion,
) -> Option<ast::Program> {
    let mut map = SourceMap::new();
    let file = match map.add_file(path.to_string_lossy().into_owned(), src.to_string()) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("topaz: {e}");
            return None;
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
    if has_errors(&out.diagnostics) {
        None
    } else {
        Some(out.program)
    }
}

pub(super) fn missing_match_case_edits(
    src: &str,
    program: &ast::Program,
    diagnostics: &[Diagnostic],
) -> Result<Vec<SourceInsert>, String> {
    let enum_payloads = enum_variant_payload_counts(src, program);
    let mut edits = Vec::new();
    for diag in diagnostics {
        let missing = parse_missing_enum_variants(&diag.message).ok_or_else(|| {
            "refactor add-missing-match-cases currently supports enum variant gaps only".to_string()
        })?;
        let insert = match_closing_line_insert(src, diag.primary.span)?;
        let indent = case_indent_in_match(src, diag.primary.span).ok_or_else(|| {
            "refactor add-missing-match-cases needs a multiline match with indented case arms"
                .to_string()
        })?;
        let mut text = String::new();
        for item in missing {
            let payload_count = enum_payloads
                .get(&(item.enum_name.clone(), item.variant_name.clone()))
                .copied()
                .ok_or_else(|| {
                    format!(
                        "missing variant `{}.{}` is not declared in this file",
                        item.enum_name, item.variant_name
                    )
                })?;
            let pattern = missing_variant_pattern(&item.variant_name, payload_count);
            let _ = writeln!(text, "{indent}case {pattern} => ()");
        }
        edits.push(SourceInsert { at: insert, text });
    }
    Ok(edits)
}

pub(super) fn enum_variant_payload_counts(
    src: &str,
    program: &ast::Program,
) -> BTreeMap<(String, String), usize> {
    let mut out = BTreeMap::new();
    for stmt in &program.items {
        collect_enum_variant_payload_counts(src, stmt, &mut out);
    }
    out
}

pub(super) fn collect_enum_variant_payload_counts(
    src: &str,
    stmt: &ast::Stmt,
    out: &mut BTreeMap<(String, String), usize>,
) {
    match &stmt.kind {
        ast::StmtKind::Export(inner) => collect_enum_variant_payload_counts(src, inner, out),
        ast::StmtKind::Enum(decl) => {
            let Some(enum_name) = source_span_text(src, decl.name.span) else {
                return;
            };
            for variant in &decl.variants {
                let Some(variant_name) = source_span_text(src, variant.name.span) else {
                    continue;
                };
                out.insert(
                    (enum_name.to_string(), variant_name.to_string()),
                    variant.payload.as_ref().map_or(0, Vec::len),
                );
            }
        }
        _ => {}
    }
}

pub(super) fn parse_missing_enum_variants(message: &str) -> Option<Vec<MissingEnumVariant>> {
    let rest = message.strip_prefix("non-exhaustive match: missing ")?;
    let mut out = Vec::new();
    for raw in rest.split(',') {
        let item = raw.trim().trim_matches('`');
        let (enum_name, variant_name) = item.split_once('.')?;
        if enum_name.is_empty() || variant_name.is_empty() {
            return None;
        }
        out.push(MissingEnumVariant {
            enum_name: enum_name.to_string(),
            variant_name: variant_name.to_string(),
        });
    }
    Some(out)
}

pub(super) fn missing_variant_pattern(variant: &str, payload_count: usize) -> String {
    if payload_count == 0 {
        return variant.to_string();
    }
    let args = (0..payload_count)
        .map(|_| "_")
        .collect::<Vec<_>>()
        .join(", ");
    format!("{variant}({args})")
}

pub(super) fn match_closing_line_insert(src: &str, span: Span) -> Result<usize, String> {
    let hi = span.hi as usize;
    let Some(prefix) = src.get(..hi) else {
        return Err("non-exhaustive match span is outside the source text".to_string());
    };
    let Some(close) = prefix
        .char_indices()
        .rev()
        .find_map(|(idx, ch)| (!ch.is_whitespace()).then_some((idx, ch)))
    else {
        return Err("non-exhaustive match span has no closing brace".to_string());
    };
    let (close, ch) = close;
    if ch != '}' {
        return Err("non-exhaustive match span does not end at a closing brace".to_string());
    }
    let line_start = src[..close].rfind('\n').map_or(0, |idx| idx + 1);
    if !src[line_start..close].trim().is_empty() {
        return Err(
            "refactor add-missing-match-cases only rewrites multiline matches whose closing brace is on its own line"
                .to_string(),
        );
    }
    Ok(line_start)
}

pub(super) fn case_indent_in_match(src: &str, span: Span) -> Option<String> {
    let start = span.lo as usize;
    let end = span.hi as usize;
    let body = src.get(start..end)?;
    for line in body.lines() {
        let trimmed = line.trim_start_matches([' ', '\t']);
        if trimmed.starts_with("case ") {
            let indent_len = line.len() - trimmed.len();
            return Some(line[..indent_len].to_string());
        }
    }
    None
}

pub(super) fn insert_source_text(src: &str, edits: &[SourceInsert]) -> String {
    let mut edits = edits.to_vec();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.at));
    let mut out = src.to_string();
    for edit in edits {
        out.insert_str(edit.at, &edit.text);
    }
    out
}

pub(super) fn refactor_entry_path(
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    entry: Option<&str>,
    default_version: LangVersion,
) -> Result<(PathBuf, LangVersion), ExitCode> {
    if let Some(entry) = entry {
        let entry = entry.replace('\\', "/");
        let (base, entry_rel, _) = match split_absolute(&entry, root) {
            Ok(v) => v,
            Err(msg) => {
                eprintln!("topaz: {msg}");
                return Err(ExitCode::FAILURE);
            }
        };
        return Ok((Path::new(&base).join(entry_rel), default_version));
    }

    let project = match topaz_package::Project::load(root.unwrap_or(".")) {
        Ok(project) => project,
        Err(e) => {
            eprintln!("topaz: {e}");
            return Err(ExitCode::FAILURE);
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
        return Err(ExitCode::FAILURE);
    }
    let path = project.root.join(&project.manifest.package.entry);
    Ok((path, project.manifest.package.language))
}

pub(super) fn source_span_text(src: &str, span: Span) -> Option<&str> {
    src.get(span.lo as usize..span.hi as usize)
}

pub(super) fn replace_source_spans(src: &str, spans: &[Span], new_text: &str) -> String {
    let mut out = src.to_string();
    for span in spans.iter().rev() {
        out.replace_range(span.lo as usize..span.hi as usize, new_text);
    }
    out
}

pub(super) fn organize_imports_source_text(src: &str) -> String {
    if src.is_empty() {
        return String::new();
    }
    let trailing_newline = src.ends_with('\n');
    let mut lines: Vec<String> = src.lines().map(str::to_string).collect();
    organize_import_lines(&mut lines);
    let mut out = lines.join("\n");
    if trailing_newline {
        out.push('\n');
    }
    out
}

pub(super) fn organize_import_lines(lines: &mut [String]) {
    let mut i = 0usize;
    while i < lines.len() {
        while i < lines.len() && lines[i].trim().is_empty() {
            i += 1;
        }
        if i >= lines.len() || !lines[i].starts_with("import ") {
            break;
        }
        let start = i;
        while i < lines.len() && lines[i].starts_with("import ") {
            i += 1;
        }
        lines[start..i].sort();
    }
}
