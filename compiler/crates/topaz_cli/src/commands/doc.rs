use crate::*;

pub(super) fn doc_package_target(
    target: &PackageTarget,
    out_dir: Option<&str>,
    compiler: CompilerSelection,
) -> ExitCode {
    let Some(out_dir) = out_dir else {
        eprintln!("topaz: `doc` requires --out-dir <dir>\n\n{USAGE}");
        return ExitCode::FAILURE;
    };
    if compiler == CompilerSelection::SelfHosted {
        return doc_self_package_target(target, out_dir);
    }
    let out = resolve_package_target(target);
    for diag in &out.diagnostics {
        eprintln!("{}", render(diag, &out.map));
    }
    if has_errors(&out.diagnostics) {
        return ExitCode::FAILURE;
    }
    let checked = match check_resolved_unit(&out, false, target.version) {
        Ok(checked) => checked,
        Err(n) => {
            eprintln!(
                "{}: {n} type diagnostic{}",
                target.entry,
                if n == 1 { "" } else { "s" }
            );
            return ExitCode::FAILURE;
        }
    };
    let out_dir = PathBuf::from(out_dir);
    if let Err(e) = fs::create_dir_all(&out_dir) {
        eprintln!("topaz: cannot create `{}`: {e}", out_dir.to_string_lossy());
        return ExitCode::FAILURE;
    }
    let doc_comments = collect_doc_comments(&out);
    let exports_json = render_export_surface_json(&checked.exports);
    let index_md = render_docs_markdown(target, &checked.exports, &doc_comments);
    let exports_path = out_dir.join("exports.json");
    let index_path = out_dir.join("index.md");
    if let Err(e) = fs::write(&exports_path, exports_json) {
        eprintln!(
            "topaz: cannot write `{}`: {e}",
            exports_path.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }
    if let Err(e) = fs::write(&index_path, index_md) {
        eprintln!(
            "topaz: cannot write `{}`: {e}",
            index_path.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }
    eprintln!("topaz: wrote docs to `{}`", out_dir.to_string_lossy());
    ExitCode::SUCCESS
}

pub(super) fn doc_self_package_target(target: &PackageTarget, out_dir: &str) -> ExitCode {
    let product = match compile_self_package_product(target, None, "doc") {
        Ok(product) => product,
        Err(code) => return code,
    };
    if product.status() != "completed" {
        return check_self_compilation_product(
            product,
            &target.entry,
            false,
            false,
            CheckPresentation::Package,
        );
    }
    let exports_json = render_self_export_surface_json(&product);
    let index_md = match render_self_docs_markdown(target, &product) {
        Ok(markdown) => markdown,
        Err(error) => {
            eprintln!("topaz: self documentation product is invalid: {error}");
            eprintln!("topaz: recovery: rerun `topaz doc --compiler rust` (not executed)");
            return ExitCode::FAILURE;
        }
    };
    let out_dir = PathBuf::from(out_dir);
    if let Err(error) = fs::create_dir_all(&out_dir) {
        eprintln!(
            "topaz: cannot create `{}`: {error}",
            out_dir.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }
    for (path, bytes) in [
        (out_dir.join("exports.json"), exports_json.as_bytes()),
        (out_dir.join("index.md"), index_md.as_bytes()),
    ] {
        if let Err(error) = fs::write(&path, bytes) {
            eprintln!("topaz: cannot write `{}`: {error}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    }
    eprintln!("topaz: wrote docs to `{}`", out_dir.to_string_lossy());
    ExitCode::SUCCESS
}

pub(super) fn self_ast_children<'a>(
    module: &'a topaz_kernel::CanonicalPreviewModule,
    parent: usize,
    field: &str,
) -> Vec<(usize, &'a topaz_kernel::CanonicalPreviewAstNode)> {
    let mut children = module
        .ast
        .iter()
        .enumerate()
        .filter(|(_, node)| node.parent == Some(parent as u32) && node.field == field)
        .collect::<Vec<_>>();
    children.sort_by_key(|(_, node)| node.index);
    children
}

pub(super) fn self_ast_text<'a>(
    module: &'a topaz_kernel::CanonicalPreviewModule,
    node: &topaz_kernel::CanonicalPreviewAstNode,
) -> Result<&'a str, String> {
    module
        .source
        .get(node.lo as usize..node.hi as usize)
        .ok_or_else(|| {
            format!(
                "module `{}` carries an invalid AST span {}..{}",
                module.identity, node.lo, node.hi
            )
        })
}

pub(super) fn self_ast_declaration_index(
    module: &topaz_kernel::CanonicalPreviewModule,
    lo: u32,
    hi: u32,
) -> Option<usize> {
    let name = module.ast.iter().position(|node| {
        node.kind == "identifier" && node.field == "name" && node.lo == lo && node.hi == hi
    })?;
    let mut index = module.ast.get(name)?.parent? as usize;
    while let Some(parent) = module.ast.get(index)?.parent {
        if module.ast[index].kind.starts_with("statement/")
            || module.ast[index].kind == "function-declaration"
        {
            break;
        }
        index = parent as usize;
    }
    Some(index)
}

pub(super) fn self_ast_named_child<'a>(
    module: &'a topaz_kernel::CanonicalPreviewModule,
    parent: usize,
    field: &str,
) -> Result<&'a topaz_kernel::CanonicalPreviewAstNode, String> {
    let children = self_ast_children(module, parent, field);
    if children.len() != 1 {
        return Err(format!(
            "module `{}` declaration has {} `{field}` children",
            module.identity,
            children.len()
        ));
    }
    Ok(children[0].1)
}

pub(super) fn self_doc_comment(
    module: &topaz_kernel::CanonicalPreviewModule,
    declaration: usize,
) -> Option<String> {
    let mut index = declaration;
    while let Some(parent) = module.ast.get(index)?.parent {
        index = parent as usize;
    }
    leading_doc_comment(&module.source, module.ast.get(index)?.lo)
}

pub(super) fn render_self_docs_markdown(
    target: &PackageTarget,
    product: &topaz_self_frontend::SelfCompilationProduct,
) -> Result<String, String> {
    let mut out = String::new();
    writeln!(
        out,
        "# {} {}\n",
        target.package_name, target.package_version
    )
    .map_err(|error| error.to_string())?;
    writeln!(out, "- Entry: `{}`", target.entry).map_err(|error| error.to_string())?;
    writeln!(
        out,
        "- Language: `{}`\n\n## Modules",
        lang_version_text(target.version)
    )
    .map_err(|error| error.to_string())?;

    for (module_index, module) in product.typed().resolved.modules.iter().enumerate() {
        let exports = product
            .typed()
            .resolved
            .exports
            .iter()
            .filter(|export| export.module_index == module_index)
            .collect::<Vec<_>>();
        if exports.is_empty() {
            continue;
        }
        let mut entries = Vec::with_capacity(exports.len());
        for export in exports {
            let declaration_index =
                self_ast_declaration_index(module, export.declaration_lo, export.declaration_hi)
                    .ok_or_else(|| {
                        format!(
                            "self documentation export `{}` has no AST declaration",
                            export.name
                        )
                    })?;
            let declaration_node = &module.ast[declaration_index];
            let category = match (export.namespace.as_str(), declaration_node.kind.as_str()) {
                ("value", _) => 0_u8,
                ("type", "statement/type-alias") => 1,
                ("type", "statement/record") => 2,
                ("type", "statement/enum") => 3,
                ("type", "statement/newtype") => 4,
                _ => {
                    return Err(format!(
                        "self documentation export `{}` has unsupported namespace `{}` and AST kind `{}`",
                        export.name, export.namespace, declaration_node.kind
                    ));
                }
            };
            entries.push((category, export, declaration_index));
        }
        entries.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.name.cmp(&right.1.name))
        });
        out.push('\n');
        push_markdown_code_heading(&mut out, "###", &module.identity);
        let mut last_category = None;
        for (category, export, declaration_index) in entries {
            let declaration = product
                .typed()
                .resolved
                .declarations
                .iter()
                .find(|declaration| {
                    declaration.module_index == module_index
                        && declaration.namespace == export.namespace
                        && declaration.lo == export.declaration_lo
                        && declaration.hi == export.declaration_hi
                })
                .ok_or_else(|| {
                    format!(
                        "self documentation export `{}` has no declaration",
                        export.name
                    )
                })?;
            let declaration_node = &module.ast[declaration_index];
            let doc = self_doc_comment(module, declaration_index);
            if last_category != Some(category) {
                out.push_str(match category {
                    0 => "\n#### Values\n",
                    1 => "\n#### Type Aliases\n",
                    2 => "\n#### Records\n",
                    3 => "\n#### Enums\n",
                    4 => "\n#### Newtypes\n",
                    _ => unreachable!("self documentation category is bounded"),
                });
                last_category = Some(category);
            }
            match (export.namespace.as_str(), declaration_node.kind.as_str()) {
                ("value", _) => {
                    out.push_str("- ");
                    push_markdown_code(&mut out, &export.name);
                    out.push_str(": ");
                    let ty = product
                        .typed()
                        .exported_value_node(export)
                        .map(|node| render_semantic_type(&node.ty))
                        .unwrap_or_else(|| "?".to_string());
                    push_markdown_code(&mut out, &ty);
                    out.push('\n');
                }
                ("type", "statement/type-alias") => {
                    out.push_str("- ");
                    push_markdown_code(&mut out, &export.name);
                    out.push_str(" = ");
                    let body = self_ast_named_child(module, declaration_index, "type")?;
                    push_markdown_code(&mut out, self_ast_text(module, body)?);
                    out.push('\n');
                }
                ("type", "statement/record") => {
                    out.push_str("- ");
                    push_markdown_code(&mut out, &export.name);
                    out.push('\n');
                    for (field_index, _) in self_ast_children(module, declaration_index, "fields") {
                        let name = self_ast_named_child(module, field_index, "name")?;
                        let ty = self_ast_named_child(module, field_index, "type")?;
                        out.push_str("  - ");
                        push_markdown_code(&mut out, self_ast_text(module, name)?);
                        out.push_str(": ");
                        push_markdown_code(&mut out, self_ast_text(module, ty)?);
                        if !self_ast_children(module, field_index, "default").is_empty() {
                            out.push_str(" = default");
                        }
                        out.push('\n');
                    }
                }
                ("type", "statement/enum") => {
                    out.push_str("- ");
                    push_markdown_code(&mut out, &export.name);
                    out.push('\n');
                    for (variant_index, _) in
                        self_ast_children(module, declaration_index, "variants")
                    {
                        let name = self_ast_named_child(module, variant_index, "name")?;
                        out.push_str("  - ");
                        push_markdown_code(&mut out, self_ast_text(module, name)?);
                        let payloads = self_ast_children(module, variant_index, "payload");
                        if !payloads.is_empty() {
                            out.push('(');
                            for (index, (_, payload)) in payloads.iter().enumerate() {
                                if index > 0 {
                                    out.push_str(", ");
                                }
                                push_markdown_code(&mut out, self_ast_text(module, payload)?);
                            }
                            out.push(')');
                        }
                        out.push('\n');
                    }
                }
                ("type", "statement/newtype") => {
                    out.push_str("- ");
                    push_markdown_code(&mut out, &export.name);
                    out.push_str(" = ");
                    let base = self_ast_named_child(module, declaration_index, "base")?;
                    push_markdown_code(&mut out, self_ast_text(module, base)?);
                    out.push('\n');
                }
                _ => {
                    return Err(format!(
                        "self documentation export `{}` has unsupported {} `{}`",
                        export.name, declaration.declaration_kind, declaration_node.kind
                    ));
                }
            }
            if let Some(doc) = doc {
                for line in doc.lines() {
                    out.push_str("  ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
    }
    Ok(out)
}

pub(super) type DocComments = BTreeMap<String, BTreeMap<String, String>>;

pub(super) fn collect_doc_comments(out: &topaz_resolve::ResolveOutput) -> DocComments {
    let mut docs = BTreeMap::new();
    for module in &out.modules {
        let src = out.map.file(module.file).src();
        let mut module_docs = BTreeMap::new();
        for stmt in &module.program.items {
            let ast::StmtKind::Export(inner) = &stmt.kind else {
                continue;
            };
            let Some(name) = exported_doc_name(inner, src) else {
                continue;
            };
            if let Some(doc) = leading_doc_comment(src, stmt.span.lo) {
                module_docs.insert(name, doc);
            }
        }
        if !module_docs.is_empty() {
            docs.insert(module.identity.clone(), module_docs);
        }
    }
    docs
}

pub(super) fn exported_doc_name(stmt: &ast::Stmt, src: &str) -> Option<String> {
    match &stmt.kind {
        ast::StmtKind::Function(decl) => Some(span_text(src, decl.name.span).to_string()),
        ast::StmtKind::Let { pattern, .. } => match &pattern.kind {
            ast::PatternKind::Binding(name) | ast::PatternKind::Typed { name, .. } => {
                Some(span_text(src, name.span).to_string())
            }
            _ => None,
        },
        ast::StmtKind::Const { name, .. } => Some(span_text(src, name.span).to_string()),
        ast::StmtKind::TypeAlias(alias) => Some(span_text(src, alias.name.span).to_string()),
        ast::StmtKind::Record(decl) => Some(span_text(src, decl.name.span).to_string()),
        ast::StmtKind::Enum(decl) => Some(span_text(src, decl.name.span).to_string()),
        ast::StmtKind::Newtype(decl) => Some(span_text(src, decl.name.span).to_string()),
        _ => None,
    }
}

pub(super) fn leading_doc_comment(src: &str, item_lo: u32) -> Option<String> {
    let prefix = src
        .get(..item_lo as usize)?
        .trim_end_matches([' ', '\t', '\r']);
    let mut lines = Vec::new();
    let mut saw_adjacent_doc = false;
    for raw_line in prefix.lines().rev() {
        let trimmed = raw_line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("///") {
            saw_adjacent_doc = true;
            lines.push(rest.trim_start().to_string());
            continue;
        }
        break;
    }
    if !saw_adjacent_doc {
        return None;
    }
    lines.reverse();
    Some(lines.join("\n"))
}

pub(super) fn render_docs_markdown(
    target: &PackageTarget,
    exports: &BTreeMap<String, topaz_check::ModuleExports>,
    docs: &DocComments,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# {} {}\n",
        target.package_name, target.package_version
    );
    let _ = writeln!(out, "- Entry: `{}`", target.entry);
    let _ = writeln!(out, "- Language: `{}`", lang_version_text(target.version));
    out.push_str("\n## Modules\n");
    for (identity, surface) in exports {
        out.push('\n');
        push_markdown_code_heading(&mut out, "###", identity);
        if surface.ambient {
            out.push_str("\nAmbient export surface.\n");
            continue;
        }
        let module_docs = docs.get(identity);
        render_docs_values(&mut out, surface, module_docs);
        render_docs_aliases(&mut out, surface, module_docs);
        render_docs_records(&mut out, surface, module_docs);
        render_docs_enums(&mut out, surface, module_docs);
        render_docs_newtypes(&mut out, surface, module_docs);
        render_docs_conformances(&mut out, surface);
    }
    out
}

pub(super) fn render_docs_values(
    out: &mut String,
    surface: &topaz_check::ModuleExports,
    docs: Option<&BTreeMap<String, String>>,
) {
    let mut values: Vec<_> = surface.values.iter().collect();
    values.sort_by_key(|(name, _)| *name);
    if values.is_empty() {
        return;
    }
    out.push_str("\n#### Values\n");
    for (name, value) in values {
        out.push_str("- ");
        push_markdown_code(out, name);
        out.push_str(": ");
        push_markdown_code(out, &value.ty.to_string());
        if value.names_known && !value.names.is_empty() {
            let _ = write!(out, " ({} required", value.required);
            if value.required != 1 {
                out.push('s');
            }
            out.push_str("; params ");
            for (i, name) in value.names.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                push_markdown_code(out, name);
                if value.defaulted.get(i).copied().unwrap_or(false) {
                    out.push_str(" = default");
                }
            }
            out.push(')');
        }
        out.push('\n');
        push_doc_comment(out, docs, name, "  ");
    }
}

pub(super) fn render_docs_aliases(
    out: &mut String,
    surface: &topaz_check::ModuleExports,
    docs: Option<&BTreeMap<String, String>>,
) {
    let mut aliases: Vec<_> = surface.aliases.iter().collect();
    aliases.sort_by_key(|(name, _)| *name);
    if aliases.is_empty() {
        return;
    }
    out.push_str("\n#### Type Aliases\n");
    for (name, alias) in aliases {
        out.push_str("- ");
        push_markdown_code(out, name);
        push_type_params_suffix(out, alias.params);
        out.push_str(" = ");
        push_markdown_code(out, &alias.body.to_string());
        out.push('\n');
        push_doc_comment(out, docs, name, "  ");
    }
}

pub(super) fn render_docs_records(
    out: &mut String,
    surface: &topaz_check::ModuleExports,
    docs: Option<&BTreeMap<String, String>>,
) {
    let mut records: Vec<_> = surface.records.iter().collect();
    records.sort_by_key(|(name, _)| *name);
    if records.is_empty() {
        return;
    }
    out.push_str("\n#### Records\n");
    for (name, record) in records {
        out.push_str("- ");
        push_markdown_code(out, name);
        push_type_params_suffix(out, record.params);
        out.push('\n');
        push_doc_comment(out, docs, name, "  ");
        for field in &record.fields {
            out.push_str("  - ");
            push_markdown_code(out, &field.name);
            out.push_str(": ");
            push_markdown_code(out, &field.ty.to_string());
            if field.has_default {
                out.push_str(" = default");
            }
            out.push('\n');
        }
    }
}

pub(super) fn render_docs_enums(
    out: &mut String,
    surface: &topaz_check::ModuleExports,
    docs: Option<&BTreeMap<String, String>>,
) {
    let mut enums: Vec<_> = surface.enums.iter().collect();
    enums.sort_by_key(|(name, _)| *name);
    if enums.is_empty() {
        return;
    }
    out.push_str("\n#### Enums\n");
    for (name, enm) in enums {
        out.push_str("- ");
        push_markdown_code(out, name);
        push_type_params_suffix(out, enm.params);
        out.push('\n');
        push_doc_comment(out, docs, name, "  ");
        for variant in &enm.variants {
            out.push_str("  - ");
            push_markdown_code(out, &variant.name);
            if !variant.payloads.is_empty() {
                out.push('(');
                for (i, payload) in variant.payloads.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    push_markdown_code(out, &payload.to_string());
                }
                out.push(')');
            }
            out.push('\n');
        }
    }
}

pub(super) fn render_docs_newtypes(
    out: &mut String,
    surface: &topaz_check::ModuleExports,
    docs: Option<&BTreeMap<String, String>>,
) {
    let mut newtypes: Vec<_> = surface.newtypes.iter().collect();
    newtypes.sort_by_key(|(name, _)| *name);
    if newtypes.is_empty() {
        return;
    }
    out.push_str("\n#### Newtypes\n");
    for (name, newtype) in newtypes {
        out.push_str("- ");
        push_markdown_code(out, name);
        push_type_params_suffix(out, newtype.params);
        out.push_str(" = ");
        push_markdown_code(out, &newtype.base.to_string());
        out.push('\n');
        push_doc_comment(out, docs, name, "  ");
    }
}

pub(super) fn push_type_params_suffix(out: &mut String, params: usize) {
    if params > 0 {
        let _ = write!(out, " ({params} type params)");
    }
}

pub(super) fn push_doc_comment(
    out: &mut String,
    docs: Option<&BTreeMap<String, String>>,
    name: &str,
    indent: &str,
) {
    let Some(doc) = docs.and_then(|docs| docs.get(name)) else {
        return;
    };
    for line in doc.lines() {
        out.push_str(indent);
        out.push_str(line);
        out.push('\n');
    }
}

pub(super) fn render_docs_conformances(out: &mut String, surface: &topaz_check::ModuleExports) {
    let mut conformances = surface.conformances.clone();
    conformances.sort();
    conformances.dedup();
    if conformances.is_empty() {
        return;
    }
    out.push_str("\n#### Conformances\n");
    for (protocol, ty) in conformances {
        out.push_str("- ");
        push_markdown_code(out, &ty);
        out.push_str(": ");
        push_markdown_code(out, &protocol);
        out.push('\n');
    }
}

pub(super) fn push_markdown_code_heading(out: &mut String, level: &str, raw: &str) {
    out.push_str(level);
    out.push(' ');
    push_markdown_code(out, raw);
    out.push('\n');
}

pub(super) fn push_markdown_code(out: &mut String, raw: &str) {
    out.push('`');
    for ch in raw.chars() {
        if ch == '`' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('`');
}
