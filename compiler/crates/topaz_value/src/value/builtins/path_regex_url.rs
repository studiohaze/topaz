use super::super::*;

pub(in crate::value) fn fs_bytes_arg(
    arg: Value,
    name: &str,
    span: Span,
) -> Result<Rc<[u8]>, RtError> {
    match arg {
        Value::Bytes(b) => Ok(b),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`FS.{name}` takes `Bytes`, found `{}`", other.kind()),
            span,
        )),
    }
}

pub(in crate::value) fn fs_dir_entry_value(entry: HostDirEntry) -> Value {
    Value::record(vec![
        ("kind".to_string(), Value::str(entry.kind)),
        ("name".to_string(), Value::str(entry.name)),
        (
            "sizeBytes".to_string(),
            match entry.size_bytes {
                Some(n) => Value::Some(Rc::new(Value::Int(n))),
                None => Value::None,
            },
        ),
    ])
}

pub(in crate::value) fn fs_path_arg(
    arg: Value,
    method: &str,
    span: Span,
) -> Result<Rc<str>, RtError> {
    match arg {
        Value::Str(path) | Value::Path(path) => Ok(path),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`FS.{method}` parameter `path` expects string or Path; found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

/// §10 `FS.readText(path) -> Result<string,string>` — explicit host file effect,
/// capability-gated by the host implementation.
pub fn builtin_fs_read_text(host: &dyn Host, path: Value, span: Span) -> Result<Value, RtError> {
    let path = fs_path_arg(path, "readText", span)?;
    Ok(
        match host.open(&path).and_then(|handle| {
            let text = host.read(handle);
            host.close(handle);
            text
        }) {
            Ok(text) => Value::Ok(Rc::new(Value::str(text))),
            Err(message) => err_string(message),
        },
    )
}

/// §10 `FS.writeText(path, text) -> Result<(),string>`.
pub fn builtin_fs_write_text(
    host: &dyn Host,
    path: Value,
    text: Value,
    span: Span,
) -> Result<Value, RtError> {
    let path = fs_path_arg(path, "writeText", span)?;
    let text = stdlib_string_arg(text, "FS", "writeText", "text", span)?;
    Ok(
        match host.open(&path).and_then(|handle| {
            let written = host.write(handle, &text);
            host.close(handle);
            written
        }) {
            Ok(()) => Value::Ok(Rc::new(Value::Unit)),
            Err(message) => err_string(message),
        },
    )
}

/// §10 `FS.readBytes(path) -> Result<Bytes,string>`.
pub fn builtin_fs_read_bytes(host: &dyn Host, path: Value, span: Span) -> Result<Value, RtError> {
    let path = fs_path_arg(path, "readBytes", span)?;
    Ok(match host.read_bytes(&path) {
        Ok(bytes) => Value::Ok(Rc::new(Value::Bytes(Rc::from(bytes.as_slice())))),
        Err(message) => err_string(message),
    })
}

/// §10 `FS.writeBytes(path, bytes) -> Result<(),string>`.
pub fn builtin_fs_write_bytes(
    host: &dyn Host,
    path: Value,
    bytes: Value,
    span: Span,
) -> Result<Value, RtError> {
    let path = fs_path_arg(path, "writeBytes", span)?;
    let bytes = fs_bytes_arg(bytes, "writeBytes", span)?;
    Ok(match host.write_bytes(&path, &bytes) {
        Ok(()) => Value::Ok(Rc::new(Value::Unit)),
        Err(message) => err_string(message),
    })
}

/// §10 `FS.list(path) -> Result<Array<{name,kind,sizeBytes}>,string>`.
pub fn builtin_fs_list(host: &dyn Host, path: Value, span: Span) -> Result<Value, RtError> {
    let path = fs_path_arg(path, "list", span)?;
    Ok(match host.list_dir(&path) {
        Ok(entries) => Value::Ok(Rc::new(Value::array(
            entries.into_iter().map(fs_dir_entry_value).collect(),
        ))),
        Err(message) => err_string(message),
    })
}

// §10/§17 (v5.4) `Cli` + `Path` stdlib leaves. These are pure deterministic
// helpers: no ambient argv/cwd/fs. The caller passes `args`, and `Path` is a
// normalized logical project-relative value that future FS leaves can trust.

pub(in crate::value) fn cli_args_arg(
    arg: Value,
    name: &str,
    span: Span,
) -> Result<Vec<Rc<str>>, RtError> {
    match arg {
        Value::Array(items) => items
            .borrow()
            .iter()
            .map(|v| match v {
                Value::Str(s) => Ok(s.clone()),
                other => Err(fault(
                    codes::GUARD_TYPE,
                    format!(
                        "`Cli.{name}` takes `Array<string>`; found `{}`",
                        other.kind()
                    ),
                    span,
                )),
            })
            .collect(),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`Cli.{name}` takes `Array<string>`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub(in crate::value) fn stdlib_string_arg(
    arg: Value,
    owner: &str,
    name: &str,
    param: &str,
    span: Span,
) -> Result<Rc<str>, RtError> {
    match arg {
        Value::Str(s) => Ok(s),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`{owner}.{name}` takes `{param}: string`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub(in crate::value) fn cli_option_values(args: &[Rc<str>], name: &str) -> Vec<Rc<str>> {
    let eq_prefix = format!("{name}=");
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_ref();
        if arg == "--" {
            break;
        }
        if let Some(rest) = arg.strip_prefix(&eq_prefix) {
            out.push(Rc::from(rest));
            i += 1;
            continue;
        }
        if arg == name {
            if let Some(next) = args.get(i + 1)
                && (!next.as_ref().starts_with('-') || next.as_ref() == "-")
            {
                out.push(next.clone());
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    out
}

pub fn builtin_cli_has_flag(args: Value, name: Value, span: Span) -> Result<Value, RtError> {
    let args = cli_args_arg(args, "hasFlag", span)?;
    let name = stdlib_string_arg(name, "Cli", "hasFlag", "name", span)?;
    Ok(Value::Bool(
        args.iter()
            .take_while(|a| a.as_ref() != "--")
            .any(|a| a.as_ref() == name.as_ref()),
    ))
}

pub fn builtin_cli_option(args: Value, name: Value, span: Span) -> Result<Value, RtError> {
    let args = cli_args_arg(args, "option", span)?;
    let name = stdlib_string_arg(name, "Cli", "option", "name", span)?;
    match cli_option_values(&args, &name).into_iter().next() {
        Some(v) => Ok(Value::Some(Rc::new(Value::Str(v)))),
        None => Ok(Value::None),
    }
}

pub fn builtin_cli_options(args: Value, name: Value, span: Span) -> Result<Value, RtError> {
    let args = cli_args_arg(args, "options", span)?;
    let name = stdlib_string_arg(name, "Cli", "options", "name", span)?;
    Ok(Value::array(
        cli_option_values(&args, &name)
            .into_iter()
            .map(Value::Str)
            .collect(),
    ))
}

pub fn builtin_cli_positionals(args: Value, span: Span) -> Result<Value, RtError> {
    let args = cli_args_arg(args, "positionals", span)?;
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut after_dashdash = false;
    while i < args.len() {
        let arg = args[i].as_ref();
        if after_dashdash {
            out.push(Value::Str(args[i].clone()));
            i += 1;
            continue;
        }
        if arg == "--" {
            after_dashdash = true;
            i += 1;
            continue;
        }
        if arg.starts_with('-') && arg != "-" {
            if !arg.contains('=')
                && args
                    .get(i + 1)
                    .is_some_and(|next| !next.as_ref().starts_with('-') || next.as_ref() == "-")
            {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        out.push(Value::Str(args[i].clone()));
        i += 1;
    }
    Ok(Value::array(out))
}

pub(in crate::value) fn normalize_path_text(text: &str) -> Result<Rc<str>, String> {
    if text.is_empty() {
        return Err("Path.from: empty path".to_string());
    }
    if text.contains('\0') {
        return Err("Path.from: path contains NUL".to_string());
    }
    let logical = text.replace('\\', "/");
    let bytes = logical.as_bytes();
    if logical.starts_with('/') || bytes.get(1) == Some(&b':') {
        return Err("Path.from: absolute paths are not allowed".to_string());
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in logical.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("Path.from: path escapes the project root".to_string());
                }
            }
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        Ok(Rc::from("."))
    } else {
        Ok(Rc::from(parts.join("/")))
    }
}

pub(in crate::value) fn ok_path(text: Rc<str>) -> Value {
    Value::Ok(Rc::new(Value::Path(text)))
}

pub(in crate::value) fn err_string(message: String) -> Value {
    Value::Err(Rc::new(Value::str(message)))
}

pub(in crate::value) fn path_arg(
    arg: Value,
    owner: &str,
    name: &str,
    span: Span,
) -> Result<Rc<str>, RtError> {
    match arg {
        Value::Path(p) => Ok(p),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`{owner}.{name}` takes `Path`, found `{}`", other.kind()),
            span,
        )),
    }
}

pub fn builtin_path_from(text: Value, span: Span) -> Result<Value, RtError> {
    let text = stdlib_string_arg(text, "Path", "from", "text", span)?;
    Ok(match normalize_path_text(&text) {
        Ok(p) => ok_path(p),
        Err(e) => err_string(e),
    })
}

pub fn builtin_path_cwd_relative(text: Value, span: Span) -> Result<Value, RtError> {
    let text = stdlib_string_arg(text, "Path", "cwdRelative", "text", span)?;
    Ok(match normalize_path_text(&text) {
        Ok(p) => ok_path(p),
        Err(e) => err_string(e.replace("Path.from", "Path.cwdRelative")),
    })
}

pub fn builtin_path_project(text: Value, span: Span) -> Result<Value, RtError> {
    let text = stdlib_string_arg(text, "Path", "project", "text", span)?;
    Ok(match normalize_path_text(&text) {
        Ok(p) => ok_path(p),
        Err(e) => err_string(e.replace("Path.from", "Path.project")),
    })
}

pub fn builtin_path_join(path: Value, child: Value, span: Span) -> Result<Value, RtError> {
    let base = path_arg(path, "Path", "join", span)?;
    let child = stdlib_string_arg(child, "Path", "join", "child", span)?;
    let joined = if base.as_ref() == "." {
        child.to_string()
    } else {
        format!("{base}/{child}")
    };
    Ok(match normalize_path_text(&joined) {
        Ok(p) => ok_path(p),
        Err(e) => err_string(e.replace("Path.from", "Path.join")),
    })
}

pub fn builtin_path_parent(path: Value, span: Span) -> Result<Value, RtError> {
    let p = path_arg(path, "Path", "parent", span)?;
    if p.as_ref() == "." {
        return Ok(Value::None);
    }
    match p.rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => {
            Ok(Value::Some(Rc::new(Value::Path(Rc::from(parent)))))
        }
        _ => Ok(Value::None),
    }
}

pub fn builtin_path_file_name(path: Value, span: Span) -> Result<Value, RtError> {
    let p = path_arg(path, "Path", "fileName", span)?;
    if p.as_ref() == "." {
        return Ok(Value::None);
    }
    Ok(Value::Some(Rc::new(Value::Str(Rc::from(
        p.rsplit('/').next().unwrap_or(&p),
    )))))
}

pub fn builtin_path_extension(path: Value, span: Span) -> Result<Value, RtError> {
    let p = path_arg(path, "Path", "extension", span)?;
    let Some(file) = p.rsplit('/').next() else {
        return Ok(Value::None);
    };
    match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => {
            Ok(Value::Some(Rc::new(Value::Str(Rc::from(ext)))))
        }
        _ => Ok(Value::None),
    }
}

pub fn builtin_path_with_extension(path: Value, ext: Value, span: Span) -> Result<Value, RtError> {
    let p = path_arg(path, "Path", "withExtension", span)?;
    let ext = stdlib_string_arg(ext, "Path", "withExtension", "ext", span)?;
    if ext.contains('/')
        || ext.contains('\\')
        || ext.contains('\0')
        || ext.as_ref() == "."
        || ext.as_ref() == ".."
    {
        return Ok(err_string(
            "Path.withExtension: invalid extension".to_string(),
        ));
    }
    let ext = ext.trim_start_matches('.');
    if p.as_ref() == "." {
        return Ok(ok_path(p));
    }
    let (dir, file) = match p.rsplit_once('/') {
        Some((d, f)) => (Some(d), f),
        None => (None, p.as_ref()),
    };
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let next_file = if ext.is_empty() {
        stem.to_string()
    } else {
        format!("{stem}.{ext}")
    };
    let next = match dir {
        Some(d) => format!("{d}/{next_file}"),
        None => next_file,
    };
    Ok(ok_path(Rc::from(next)))
}

pub fn builtin_path_normalize(path: Value, span: Span) -> Result<Value, RtError> {
    let p = path_arg(path, "Path", "normalize", span)?;
    Ok(Value::Path(p))
}

pub fn builtin_path_to_string(path: Value, span: Span) -> Result<Value, RtError> {
    let p = path_arg(path, "Path", "toString", span)?;
    Ok(Value::Str(p))
}

// §11 (v5.4) Regex stdlib leaves. The engine operates on Unicode scalar indices
// directly; `Match.start/end` are therefore scalar offsets by construction.

pub(in crate::value) fn regex_arg(
    arg: Value,
    owner: &str,
    name: &str,
    span: Span,
) -> Result<Rc<MiniRegex>, RtError> {
    match arg {
        Value::Regex(re) => Ok(re),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`{owner}.{name}` takes `Regex`, found `{}`", other.kind()),
            span,
        )),
    }
}

pub(in crate::value) fn regex_match_groups_value(m: &RegexMatchData) -> Value {
    Value::array(
        m.groups
            .iter()
            .map(|g| match g {
                Some(s) => Value::Some(Rc::new(Value::Str(s.clone()))),
                None => Value::None,
            })
            .collect(),
    )
}

pub(in crate::value) fn regex_match_named_value(m: &RegexMatchData) -> Value {
    let mut map = OrderedMap::new();
    for (name, value) in m.named.iter() {
        map.insert(Key::Str(name.clone()), Value::Str(value.clone()));
    }
    Value::Map(Rc::new(RefCell::new(map)))
}

pub(in crate::value) fn scalar_bounds(text: &str) -> Vec<usize> {
    let mut out: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    out.push(text.len());
    out
}

pub(in crate::value) fn scalar_slice<'a>(
    text: &'a str,
    bounds: &[usize],
    start: usize,
    end: usize,
) -> &'a str {
    &text[bounds[start]..bounds[end]]
}

pub(in crate::value) fn regex_match_from_mini(
    re: &MiniRegex,
    text: &str,
    bounds: &[usize],
    m: MiniMatch,
) -> Value {
    let groups: Vec<Option<Rc<str>>> = m
        .groups
        .iter()
        .map(|g| g.map(|(s, e)| Rc::from(scalar_slice(text, bounds, s, e))))
        .collect();
    let mut named = BTreeMap::<Rc<str>, Rc<str>>::new();
    for (name, index) in re.group_names.iter() {
        if let Some((s, e)) = m.groups.get(index - 1).and_then(|g| *g) {
            named.insert(name.clone(), Rc::from(scalar_slice(text, bounds, s, e)));
        }
    }
    Value::RegexMatch(Rc::new(RegexMatchData {
        start: m.start as i64,
        end: m.end as i64,
        text: Rc::from(scalar_slice(text, bounds, m.start, m.end)),
        groups: Rc::from(groups.into_boxed_slice()),
        named: Rc::from(named.into_iter().collect::<Vec<_>>().into_boxed_slice()),
    }))
}

pub fn builtin_regex_compile(pattern: Value, span: Span) -> Result<Value, RtError> {
    let pattern = stdlib_string_arg(pattern, "Regex", "compile", "pattern", span)?;
    Ok(match MiniRegex::compile(pattern) {
        Ok(re) => Value::Ok(Rc::new(Value::Regex(Rc::new(re)))),
        Err(e) => Value::Err(Rc::new(Value::str(format!(
            "Regex.compile: invalid pattern: {e}"
        )))),
    })
}

pub fn builtin_regex_is_match(regex: Value, text: Value, span: Span) -> Result<Value, RtError> {
    let re = regex_arg(regex, "Regex", "isMatch", span)?;
    let text = stdlib_string_arg(text, "Regex", "isMatch", "text", span)?;
    re.is_match(&text)
        .map(Value::Bool)
        .map_err(|e| fault(codes::GUARD_UNIMPLEMENTED, e, span))
}

pub fn builtin_regex_find(regex: Value, text: Value, span: Span) -> Result<Value, RtError> {
    let re = regex_arg(regex, "Regex", "find", span)?;
    let text = stdlib_string_arg(text, "Regex", "find", "text", span)?;
    let chars: Vec<char> = text.chars().collect();
    let bounds = scalar_bounds(&text);
    match re
        .find_from(&chars, 0)
        .map_err(|e| fault(codes::GUARD_UNIMPLEMENTED, e, span))?
    {
        Some(m) => Ok(Value::Some(Rc::new(regex_match_from_mini(
            &re, &text, &bounds, m,
        )))),
        None => Ok(Value::None),
    }
}

pub fn builtin_regex_find_all(regex: Value, text: Value, span: Span) -> Result<Value, RtError> {
    let re = regex_arg(regex, "Regex", "findAll", span)?;
    let text = stdlib_string_arg(text, "Regex", "findAll", "text", span)?;
    let chars: Vec<char> = text.chars().collect();
    let bounds = scalar_bounds(&text);
    let mut out = Vec::new();
    let mut search = 0usize;
    while search <= chars.len() {
        match re.find_from(&chars, search) {
            Ok(Some(m)) => {
                let next = if m.end == m.start {
                    m.end.saturating_add(1)
                } else {
                    m.end
                };
                out.push(regex_match_from_mini(&re, &text, &bounds, m));
                if next > chars.len() {
                    break;
                }
                search = next;
            }
            Ok(None) => break,
            Err(e) => return Err(fault(codes::GUARD_UNIMPLEMENTED, e, span)),
        }
    }
    Ok(Value::array(out))
}

pub fn builtin_regex_split(regex: Value, text: Value, span: Span) -> Result<Value, RtError> {
    let re = regex_arg(regex, "Regex", "split", span)?;
    let text = stdlib_string_arg(text, "Regex", "split", "text", span)?;
    let chars: Vec<char> = text.chars().collect();
    let bounds = scalar_bounds(&text);
    let mut out = Vec::new();
    let mut last = 0usize;
    let mut search = 0usize;
    while search <= chars.len() {
        match re.find_from(&chars, search) {
            Ok(Some(m)) => {
                out.push(Value::str(scalar_slice(&text, &bounds, last, m.start)));
                last = m.end;
                let next = if m.end == m.start {
                    m.end.saturating_add(1)
                } else {
                    m.end
                };
                if next > chars.len() {
                    break;
                }
                search = next;
            }
            Ok(None) => break,
            Err(e) => return Err(fault(codes::GUARD_UNIMPLEMENTED, e, span)),
        }
    }
    out.push(Value::str(scalar_slice(&text, &bounds, last, chars.len())));
    Ok(Value::array(out))
}

pub fn builtin_regex_replace_all(
    regex: Value,
    text: Value,
    replacement: Value,
    span: Span,
) -> Result<Value, RtError> {
    let re = regex_arg(regex, "Regex", "replaceAll", span)?;
    let text = stdlib_string_arg(text, "Regex", "replaceAll", "text", span)?;
    let replacement = stdlib_string_arg(replacement, "Regex", "replaceAll", "replacement", span)?;
    let chars: Vec<char> = text.chars().collect();
    let bounds = scalar_bounds(&text);
    let mut out = String::new();
    let mut last = 0usize;
    let mut search = 0usize;
    while search <= chars.len() {
        match re.find_from(&chars, search) {
            Ok(Some(m)) => {
                out.push_str(scalar_slice(&text, &bounds, last, m.start));
                out.push_str(&replacement);
                last = m.end;
                let next = if m.end == m.start {
                    m.end.saturating_add(1)
                } else {
                    m.end
                };
                if next > chars.len() {
                    break;
                }
                search = next;
            }
            Ok(None) => break,
            Err(e) => return Err(fault(codes::GUARD_UNIMPLEMENTED, e, span)),
        }
    }
    out.push_str(scalar_slice(&text, &bounds, last, chars.len()));
    Ok(Value::Str(Rc::from(out)))
}

// §12/§16 (v5.4) CSV/TOML/URL stdlib leaves. These are deliberately
// dependency-free and deterministic; data errors return Result::Err strings.

pub(in crate::value) fn array_of_strings_arg(
    arg: Value,
    owner: &str,
    name: &str,
    span: Span,
) -> Result<Vec<Rc<str>>, RtError> {
    match arg {
        Value::Array(items) => items
            .borrow()
            .iter()
            .map(|v| match v {
                Value::Str(s) => Ok(s.clone()),
                other => Err(fault(
                    codes::GUARD_TYPE,
                    format!(
                        "`{owner}.{name}` takes `Array<string>`, found `{}`",
                        other.kind()
                    ),
                    span,
                )),
            })
            .collect(),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`{owner}.{name}` takes `Array<string>`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub(in crate::value) fn array_of_string_arrays_arg(
    arg: Value,
    owner: &str,
    name: &str,
    span: Span,
) -> Result<Vec<Vec<Rc<str>>>, RtError> {
    match arg {
        Value::Array(rows) => rows
            .borrow()
            .iter()
            .map(|row| array_of_strings_arg(row.clone(), owner, name, span))
            .collect(),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`{owner}.{name}` takes `Array<Array<string>>`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub(in crate::value) fn csv_parse_text(text: &str) -> Result<Vec<Vec<Rc<str>>>, String> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut in_quotes = false;
    let mut after_quote = false;
    let mut just_ended_row = false;
    while let Some(ch) = chars.next() {
        if in_quotes {
            match ch {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => {
                    in_quotes = false;
                    after_quote = true;
                }
                c => field.push(c),
            }
            just_ended_row = false;
            continue;
        }
        match ch {
            '"' if field.is_empty() && !after_quote => {
                in_quotes = true;
                just_ended_row = false;
            }
            '"' => return Err("CSV.parse: unexpected quote in unquoted field".to_string()),
            ',' => {
                row.push(Rc::from(field.as_str()));
                field.clear();
                after_quote = false;
                just_ended_row = false;
            }
            '\n' => {
                row.push(Rc::from(field.as_str()));
                rows.push(row);
                row = Vec::new();
                field.clear();
                after_quote = false;
                just_ended_row = true;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(Rc::from(field.as_str()));
                rows.push(row);
                row = Vec::new();
                field.clear();
                after_quote = false;
                just_ended_row = true;
            }
            c if after_quote && !matches!(c, ' ' | '\t') => {
                return Err("CSV.parse: quoted field must end before the delimiter".to_string());
            }
            c if after_quote => {
                field.push(c);
                just_ended_row = false;
            }
            c => {
                field.push(c);
                just_ended_row = false;
            }
        }
    }
    if in_quotes {
        return Err("CSV.parse: unterminated quoted field".to_string());
    }
    if !(just_ended_row && field.is_empty() && row.is_empty()) {
        row.push(Rc::from(field.as_str()));
        rows.push(row);
    }
    Ok(rows)
}

pub(in crate::value) fn csv_rows_value(rows: Vec<Vec<Rc<str>>>) -> Value {
    Value::array(
        rows.into_iter()
            .map(|row| Value::array(row.into_iter().map(Value::Str).collect()))
            .collect(),
    )
}

pub(in crate::value) fn csv_quote_field(out: &mut String, field: &str) {
    let quote =
        field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r');
    if quote {
        out.push('"');
        for ch in field.chars() {
            if ch == '"' {
                out.push('"');
            }
            out.push(ch);
        }
        out.push('"');
    } else {
        out.push_str(field);
    }
}

pub(in crate::value) fn csv_stringify_rows(rows: &[Vec<Rc<str>>]) -> String {
    let mut out = String::new();
    for (ri, row) in rows.iter().enumerate() {
        if ri > 0 {
            out.push('\n');
        }
        for (ci, field) in row.iter().enumerate() {
            if ci > 0 {
                out.push(',');
            }
            csv_quote_field(&mut out, field);
        }
    }
    out
}

pub fn builtin_csv_parse(text: Value, span: Span) -> Result<Value, RtError> {
    let text = stdlib_string_arg(text, "CSV", "parse", "text", span)?;
    Ok(match csv_parse_text(&text) {
        Ok(rows) => Value::Ok(Rc::new(csv_rows_value(rows))),
        Err(e) => err_string(e),
    })
}

pub fn builtin_csv_parse_with_header(text: Value, span: Span) -> Result<Value, RtError> {
    let text = stdlib_string_arg(text, "CSV", "parseWithHeader", "text", span)?;
    Ok(match csv_parse_text(&text) {
        Ok(mut rows) => {
            if rows.is_empty() {
                return Ok(Value::Ok(Rc::new(Value::array(Vec::new()))));
            }
            let header = rows.remove(0);
            let mut out = Vec::new();
            for row in rows {
                if row.len() > header.len() {
                    return Ok(err_string(
                        "CSV.parseWithHeader: row has more fields than the header".to_string(),
                    ));
                }
                let mut map = OrderedMap::new();
                for (i, name) in header.iter().enumerate() {
                    let value = row.get(i).cloned().unwrap_or_else(|| Rc::from(""));
                    map.insert(Key::Str(name.clone()), Value::Str(value));
                }
                out.push(Value::Map(Rc::new(RefCell::new(map))));
            }
            Value::Ok(Rc::new(Value::array(out)))
        }
        Err(e) => err_string(e),
    })
}

pub fn builtin_csv_stringify(rows: Value, span: Span) -> Result<Value, RtError> {
    let rows = array_of_string_arrays_arg(rows, "CSV", "stringify", span)?;
    Ok(Value::Str(Rc::from(csv_stringify_rows(&rows))))
}

pub fn builtin_csv_stringify_with_header(
    rows: Value,
    columns: Value,
    span: Span,
) -> Result<Value, RtError> {
    let columns = array_of_strings_arg(columns, "CSV", "stringifyWithHeader", span)?;
    let rows = match rows {
        Value::Array(items) => items.borrow().clone(),
        other => {
            return Err(fault(
                codes::GUARD_TYPE,
                format!(
                    "`CSV.stringifyWithHeader` takes `Array<Map<string,string>>`, found `{}`",
                    other.kind()
                ),
                span,
            ));
        }
    };
    let mut all_rows = Vec::new();
    all_rows.push(columns.clone());
    for row in rows {
        let Value::Map(map) = row else {
            return Err(fault(
                codes::GUARD_TYPE,
                "`CSV.stringifyWithHeader` takes `Array<Map<string,string>>`".to_string(),
                span,
            ));
        };
        let mut out = Vec::new();
        for col in &columns {
            let value = match map.borrow().get(&Key::Str(col.clone())) {
                Some(Value::Str(s)) => s,
                Some(other) => {
                    return Err(fault(
                        codes::GUARD_TYPE,
                        format!(
                            "`CSV.stringifyWithHeader` map values must be string, found `{}`",
                            other.kind()
                        ),
                        span,
                    ));
                }
                None => Rc::from(""),
            };
            out.push(value);
        }
        all_rows.push(out);
    }
    Ok(Value::Str(Rc::from(csv_stringify_rows(&all_rows))))
}

pub(in crate::value) fn toml_err(message: impl Into<String>, line: usize) -> String {
    format!("TOML.parse: line {line}: {}", message.into())
}

pub(in crate::value) fn strip_toml_comment(line: &str) -> String {
    let mut out = String::new();
    let mut in_str = false;
    let mut escape = false;
    for ch in line.chars() {
        if in_str {
            out.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_str = true;
                out.push(ch);
            }
            '#' => break,
            c => out.push(c),
        }
    }
    out
}

pub(in crate::value) fn split_toml_eq(line: &str) -> Option<(&str, &str)> {
    let mut in_str = false;
    let mut escape = false;
    let mut depth = 0i32;
    for (idx, ch) in line.char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            '=' if depth == 0 => return Some((&line[..idx], &line[idx + 1..])),
            _ => {}
        }
    }
    None
}

pub(in crate::value) fn parse_toml_key_path(
    input: &str,
    line: usize,
) -> Result<Vec<Rc<str>>, String> {
    let mut parts = Vec::new();
    let mut raw_parts = Vec::new();
    let mut start = 0usize;
    let mut in_str = false;
    let mut escape = false;
    for (idx, ch) in input.char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '.' => {
                raw_parts.push(&input[start..idx]);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    if in_str {
        return Err(toml_err("unterminated quoted key", line));
    }
    raw_parts.push(&input[start..]);

    for raw in raw_parts {
        let part = raw.trim();
        if part.is_empty() {
            return Err(toml_err("empty key segment", line));
        }
        if part.starts_with('"') {
            let parsed = parse_toml_string(part, line)?;
            parts.push(Rc::from(parsed.as_str()));
        } else if part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            parts.push(Rc::from(part));
        } else {
            return Err(toml_err(format!("invalid key segment `{part}`"), line));
        }
    }
    Ok(parts)
}

pub(in crate::value) fn parse_toml_string(input: &str, line: usize) -> Result<String, String> {
    let mut chars = input.chars();
    if chars.next() != Some('"') {
        return Err(toml_err("expected string", line));
    }
    let mut out = String::new();
    let mut closed = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                closed = true;
                break;
            }
            '\\' => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => {
                    return Err(toml_err(format!("unsupported escape `\\{other}`"), line));
                }
                None => return Err(toml_err("dangling string escape", line)),
            },
            c => out.push(c),
        }
    }
    if !closed || !chars.as_str().trim().is_empty() {
        return Err(toml_err("unterminated or trailing string content", line));
    }
    Ok(out)
}

pub(in crate::value) fn split_toml_array(input: &str, line: usize) -> Result<Vec<&str>, String> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut in_str = false;
    let mut escape = false;
    let mut depth = 0i32;
    for (idx, ch) in input.char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(input[start..idx].trim());
                start = idx + 1;
            }
            _ => {}
        }
    }
    if in_str || depth != 0 {
        return Err(toml_err("unterminated array value", line));
    }
    let tail = input[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    Ok(parts)
}

pub(in crate::value) fn parse_toml_inline_table(
    input: &str,
    line: usize,
) -> Result<TomlValue, String> {
    let inner = input
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .ok_or_else(|| toml_err("malformed inline table", line))?;
    let mut table = BTreeMap::new();
    for part in split_toml_array(inner, line)? {
        let Some((key_s, val_s)) = split_toml_eq(part) else {
            return Err(toml_err("expected `key = value` in inline table", line));
        };
        let mut key_path = parse_toml_key_path(key_s, line)?;
        let key = key_path.pop().ok_or_else(|| toml_err("empty key", line))?;
        let value = parse_toml_value(val_s, line)?;
        insert_toml_at(&mut table, &key_path, key, value).map_err(|e| toml_err(e, line))?;
    }
    Ok(TomlValue::Table(Rc::new(table)))
}

pub(in crate::value) fn parse_toml_value(input: &str, line: usize) -> Result<TomlValue, String> {
    let s = input.trim();
    if s.starts_with('"') {
        return Ok(TomlValue::String(Rc::from(
            parse_toml_string(s, line)?.as_str(),
        )));
    }
    if s == "true" {
        return Ok(TomlValue::Bool(true));
    }
    if s == "false" {
        return Ok(TomlValue::Bool(false));
    }
    if s.starts_with('[') && s.ends_with(']') {
        let inner = &s[1..s.len() - 1];
        let values = split_toml_array(inner, line)?
            .into_iter()
            .map(|part| parse_toml_value(part, line))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(TomlValue::Array(Rc::from(values.into_boxed_slice())));
    }
    if s.starts_with('{') && s.ends_with('}') {
        return parse_toml_inline_table(s, line);
    }
    let cleaned = s.replace('_', "");
    if cleaned.contains('.') || cleaned.contains('e') || cleaned.contains('E') {
        let parsed = cleaned
            .parse::<f64>()
            .map_err(|_| toml_err(format!("invalid float `{s}`"), line))?;
        if !parsed.is_finite() {
            return Err(toml_err("non-finite floats are not supported", line));
        }
        return Ok(TomlValue::Float(Rc::from(cleaned.as_str())));
    }
    let n = cleaned
        .parse::<i64>()
        .map_err(|_| toml_err(format!("unsupported value `{s}`"), line))?;
    Ok(TomlValue::Integer(n))
}

pub(in crate::value) fn insert_toml_at(
    table: &mut BTreeMap<Rc<str>, TomlValue>,
    path: &[Rc<str>],
    key: Rc<str>,
    value: TomlValue,
) -> Result<(), String> {
    if path.is_empty() {
        if table.contains_key(&key) {
            return Err(format!("duplicate key `{key}`"));
        }
        table.insert(key, value);
        return Ok(());
    }
    let head = path[0].clone();
    let mut child = match table.remove(&head) {
        Some(TomlValue::Table(t)) => (*t).clone(),
        Some(_) => return Err(format!("`{head}` is already a scalar value")),
        None => BTreeMap::new(),
    };
    insert_toml_at(&mut child, &path[1..], key, value)?;
    table.insert(head, TomlValue::Table(Rc::new(child)));
    Ok(())
}

pub(in crate::value) fn ensure_toml_table(
    table: &mut BTreeMap<Rc<str>, TomlValue>,
    path: &[Rc<str>],
) -> Result<(), String> {
    if path.is_empty() {
        return Ok(());
    }
    let head = path[0].clone();
    let mut child = match table.remove(&head) {
        Some(TomlValue::Table(t)) => (*t).clone(),
        Some(_) => return Err(format!("`{head}` is already a scalar value")),
        None => BTreeMap::new(),
    };
    ensure_toml_table(&mut child, &path[1..])?;
    table.insert(head, TomlValue::Table(Rc::new(child)));
    Ok(())
}

pub(in crate::value) fn append_toml_array_table(
    table: &mut BTreeMap<Rc<str>, TomlValue>,
    path: &[Rc<str>],
) -> Result<(), String> {
    let Some((head, tail)) = path.split_first() else {
        return Err("empty array-table path".to_string());
    };
    if tail.is_empty() {
        let mut items = match table.remove(head) {
            Some(TomlValue::Array(items)) => items.iter().cloned().collect::<Vec<_>>(),
            Some(_) => return Err(format!("`{head}` is already a non-array value")),
            None => Vec::new(),
        };
        items.push(TomlValue::Table(Rc::new(BTreeMap::new())));
        table.insert(
            head.clone(),
            TomlValue::Array(Rc::from(items.into_boxed_slice())),
        );
        return Ok(());
    }
    let mut child = match table.remove(head) {
        Some(TomlValue::Table(t)) => (*t).clone(),
        Some(_) => return Err(format!("`{head}` is already a scalar value")),
        None => BTreeMap::new(),
    };
    append_toml_array_table(&mut child, tail)?;
    table.insert(head.clone(), TomlValue::Table(Rc::new(child)));
    Ok(())
}

pub(in crate::value) fn insert_toml_in_array_table(
    table: &mut BTreeMap<Rc<str>, TomlValue>,
    path: &[Rc<str>],
    key_path: &[Rc<str>],
    key: Rc<str>,
    value: TomlValue,
) -> Result<(), String> {
    let Some((head, tail)) = path.split_first() else {
        return Err("empty array-table path".to_string());
    };
    if tail.is_empty() {
        let mut items = match table.remove(head) {
            Some(TomlValue::Array(items)) => items.iter().cloned().collect::<Vec<_>>(),
            Some(_) => return Err(format!("`{head}` is already a non-array value")),
            None => return Err(format!("array table `{head}` has no active item")),
        };
        let Some(last) = items.last_mut() else {
            return Err(format!("array table `{head}` has no active item"));
        };
        let mut last_table = match last {
            TomlValue::Table(t) => (**t).clone(),
            _ => return Err(format!("array table `{head}` contains a non-table item")),
        };
        insert_toml_at(&mut last_table, key_path, key, value)?;
        *last = TomlValue::Table(Rc::new(last_table));
        table.insert(
            head.clone(),
            TomlValue::Array(Rc::from(items.into_boxed_slice())),
        );
        return Ok(());
    }
    let mut child = match table.remove(head) {
        Some(TomlValue::Table(t)) => (*t).clone(),
        Some(_) => return Err(format!("`{head}` is already a scalar value")),
        None => return Err(format!("array-table parent `{head}` is missing")),
    };
    insert_toml_in_array_table(&mut child, tail, key_path, key, value)?;
    table.insert(head.clone(), TomlValue::Table(Rc::new(child)));
    Ok(())
}

pub(in crate::value) fn toml_parse_text(text: &str) -> Result<TomlValue, String> {
    let mut root = BTreeMap::new();
    let mut section: Vec<Rc<str>> = Vec::new();
    let mut section_is_array = false;
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let stripped = strip_toml_comment(raw);
        let line = stripped.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("[[") {
            if !line.ends_with("]]") {
                return Err(toml_err("unsupported or malformed table header", line_no));
            }
            section = parse_toml_key_path(&line[2..line.len() - 2], line_no)?;
            append_toml_array_table(&mut root, &section).map_err(|e| toml_err(e, line_no))?;
            section_is_array = true;
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') {
                return Err(toml_err("unsupported or malformed table header", line_no));
            }
            section = parse_toml_key_path(&line[1..line.len() - 1], line_no)?;
            ensure_toml_table(&mut root, &section).map_err(|e| toml_err(e, line_no))?;
            section_is_array = false;
            continue;
        }
        let Some((key_s, val_s)) = split_toml_eq(line) else {
            return Err(toml_err("expected `key = value`", line_no));
        };
        let mut key_path = parse_toml_key_path(key_s, line_no)?;
        let key = key_path
            .pop()
            .ok_or_else(|| toml_err("empty key", line_no))?;
        let mut full = section.clone();
        full.extend(key_path);
        let value = parse_toml_value(val_s, line_no)?;
        if section_is_array {
            insert_toml_in_array_table(&mut root, &section, &full[section.len()..], key, value)
                .map_err(|e| toml_err(e, line_no))?;
        } else {
            insert_toml_at(&mut root, &full, key, value).map_err(|e| toml_err(e, line_no))?;
        }
    }
    Ok(TomlValue::Table(Rc::new(root)))
}

pub fn toml_parse_document(text: &str) -> Result<TomlValue, String> {
    toml_parse_text(text)
}

pub(in crate::value) fn push_toml_string(out: &mut String, raw: &str) {
    out.push('"');
    for ch in raw.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub(in crate::value) fn push_toml_key(out: &mut String, raw: &str) {
    if !raw.is_empty()
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        out.push_str(raw);
    } else {
        push_toml_string(out, raw);
    }
}

pub(in crate::value) fn write_toml_inline(out: &mut String, value: &TomlValue) {
    match value {
        TomlValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        TomlValue::String(s) => push_toml_string(out, s),
        TomlValue::Integer(n) => out.push_str(&n.to_string()),
        TomlValue::Float(f) => out.push_str(f),
        TomlValue::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_toml_inline(out, item);
            }
            out.push(']');
        }
        TomlValue::Table(entries) => {
            out.push('{');
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                push_toml_key(out, k);
                out.push_str(" = ");
                write_toml_inline(out, v);
            }
            out.push('}');
        }
    }
}

pub(in crate::value) fn write_toml_document(
    out: &mut String,
    value: &TomlValue,
) -> Result<(), String> {
    let TomlValue::Table(entries) = value else {
        write_toml_inline(out, value);
        out.push('\n');
        return Ok(());
    };
    fn write_table(out: &mut String, prefix: &[Rc<str>], entries: &BTreeMap<Rc<str>, TomlValue>) {
        for (k, v) in entries {
            if !matches!(v, TomlValue::Table(_)) {
                push_toml_key(out, k);
                out.push_str(" = ");
                write_toml_inline(out, v);
                out.push('\n');
            }
        }
        for (k, v) in entries {
            if let TomlValue::Table(child) = v {
                if !out.is_empty() && !out.ends_with("\n\n") {
                    out.push('\n');
                }
                let mut next = prefix.to_vec();
                next.push(k.clone());
                out.push('[');
                for (i, part) in next.iter().enumerate() {
                    if i > 0 {
                        out.push('.');
                    }
                    push_toml_key(out, part);
                }
                out.push_str("]\n");
                write_table(out, &next, child);
            }
        }
    }
    write_table(out, &[], entries);
    Ok(())
}

pub(in crate::value) fn toml_to_json_value(value: &TomlValue) -> JsonValue {
    match value {
        TomlValue::Bool(b) => JsonValue::Bool(*b),
        TomlValue::String(s) => JsonValue::String(s.clone()),
        TomlValue::Integer(n) => JsonValue::Number(JsonNumber {
            lexeme: Rc::from(n.to_string().as_str()),
            int: Some(*n),
        }),
        TomlValue::Float(f) => JsonValue::Number(JsonNumber {
            lexeme: f.clone(),
            int: json_exact_int(f),
        }),
        TomlValue::Array(items) => JsonValue::Array(Rc::from(
            items
                .iter()
                .map(toml_to_json_value)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )),
        TomlValue::Table(entries) => JsonValue::Object(Rc::new(
            entries
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json_value(v)))
                .collect(),
        )),
    }
}

pub(in crate::value) fn json_to_toml_value(value: &JsonValue) -> Result<TomlValue, String> {
    Ok(match value {
        JsonValue::Null => return Err("TOML.fromJson: TOML has no null value".to_string()),
        JsonValue::Bool(b) => TomlValue::Bool(*b),
        JsonValue::String(s) => TomlValue::String(s.clone()),
        JsonValue::Number(n) => match n.int {
            Some(i)
                if !n.lexeme.contains('.')
                    && !n.lexeme.contains('e')
                    && !n.lexeme.contains('E') =>
            {
                TomlValue::Integer(i)
            }
            _ => {
                let parsed = n.lexeme.parse::<f64>().map_err(|_| {
                    "TOML.fromJson: number is not representable as a TOML float".to_string()
                })?;
                if !parsed.is_finite() {
                    return Err(
                        "TOML.fromJson: number is not representable as a finite TOML float"
                            .to_string(),
                    );
                }
                TomlValue::Float(n.lexeme.clone())
            }
        },
        JsonValue::Array(items) => TomlValue::Array(Rc::from(
            items
                .iter()
                .map(json_to_toml_value)
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice(),
        )),
        JsonValue::Object(entries) => TomlValue::Table(Rc::new(
            entries
                .iter()
                .map(|(k, v)| Ok((k.clone(), json_to_toml_value(v)?)))
                .collect::<Result<BTreeMap<_, _>, String>>()?,
        )),
    })
}

pub(in crate::value) fn toml_arg(
    arg: Value,
    owner: &str,
    name: &str,
    span: Span,
) -> Result<Rc<TomlValue>, RtError> {
    match arg {
        Value::Toml(t) => Ok(t),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`{owner}.{name}` takes `TOMLValue`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub fn builtin_toml_parse(text: Value, span: Span) -> Result<Value, RtError> {
    let text = stdlib_string_arg(text, "TOML", "parse", "text", span)?;
    Ok(match toml_parse_text(&text) {
        Ok(value) => Value::Ok(Rc::new(Value::Toml(Rc::new(value)))),
        Err(e) => err_string(e),
    })
}

pub fn builtin_toml_stringify(value: Value, span: Span) -> Result<Value, RtError> {
    let value = toml_arg(value, "TOML", "stringify", span)?;
    let mut out = String::new();
    Ok(match write_toml_document(&mut out, &value) {
        Ok(()) => Value::Ok(Rc::new(Value::Str(Rc::from(out)))),
        Err(e) => err_string(e),
    })
}

pub fn builtin_toml_to_json(value: Value, span: Span) -> Result<Value, RtError> {
    let value = toml_arg(value, "TOML", "toJson", span)?;
    Ok(Value::Json(Rc::new(toml_to_json_value(&value))))
}

pub fn builtin_toml_from_json(value: Value, span: Span) -> Result<Value, RtError> {
    match value {
        Value::Json(node) => Ok(match json_to_toml_value(&node) {
            Ok(v) => Value::Ok(Rc::new(Value::Toml(Rc::new(v)))),
            Err(e) => err_string(e),
        }),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!(
                "`TOML.fromJson` takes `JSONValue`, found `{}`",
                other.kind()
            ),
            span,
        )),
    }
}

pub(in crate::value) fn split_once_char(s: &str, needle: char) -> (&str, Option<&str>) {
    match s.find(needle) {
        Some(i) => (&s[..i], Some(&s[i + needle.len_utf8()..])),
        None => (s, None),
    }
}

pub(in crate::value) fn valid_url_scheme(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

pub(in crate::value) fn percent_decode_component(s: &str) -> Result<Rc<str>, String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err("URL.parse: dangling percent escape".to_string());
                }
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .map_err(|_| "URL.parse: invalid percent escape".to_string())?;
                let b = u8::from_str_radix(hex, 16)
                    .map_err(|_| "URL.parse: invalid percent escape".to_string())?;
                out.push(b);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    let decoded = String::from_utf8(out)
        .map_err(|_| "URL.parse: invalid UTF-8 percent escape".to_string())?;
    Ok(Rc::from(decoded.as_str()))
}

pub(in crate::value) fn parse_url_query(raw: Option<&str>) -> Result<UrlQueryPairs, String> {
    let Some(raw) = raw else {
        return Ok(Rc::from([] as [(Rc<str>, Rc<str>); 0]));
    };
    if raw.is_empty() {
        return Ok(Rc::from([] as [(Rc<str>, Rc<str>); 0]));
    }
    let mut pairs = Vec::new();
    for part in raw.split('&') {
        let (k, v) = split_once_char(part, '=');
        pairs.push((
            percent_decode_component(k)?,
            percent_decode_component(v.unwrap_or(""))?,
        ));
    }
    Ok(Rc::from(pairs.into_boxed_slice()))
}

pub(in crate::value) fn parse_url_text(text: &str) -> Result<UrlData, String> {
    if text.is_empty() || text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(
            "URL.parse: URL is empty or contains whitespace/control characters".to_string(),
        );
    }
    let Some(colon) = text.find(':') else {
        return Err("URL.parse: missing scheme".to_string());
    };
    let scheme_raw = &text[..colon];
    if !valid_url_scheme(scheme_raw) {
        return Err("URL.parse: invalid scheme".to_string());
    }
    let scheme = scheme_raw.to_ascii_lowercase();
    let rest = &text[colon + 1..];
    let (without_fragment, fragment_raw) = split_once_char(rest, '#');
    let (without_query, query_raw) = split_once_char(without_fragment, '?');
    let mut authority = None;
    let mut host = None;
    let path;
    if let Some(after_slashes) = without_query.strip_prefix("//") {
        let slash = after_slashes.find('/').unwrap_or(after_slashes.len());
        let auth = &after_slashes[..slash];
        if auth.is_empty() || auth.contains('@') {
            return Err("URL.parse: authority must contain a host and no userinfo".to_string());
        }
        let (host_part, port_part) = split_once_char(auth, ':');
        if host_part.is_empty() {
            return Err("URL.parse: host is empty".to_string());
        }
        if let Some(port) = port_part
            && (port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()))
        {
            return Err("URL.parse: invalid port".to_string());
        }
        let h = host_part.to_ascii_lowercase();
        let auth_canon = match port_part {
            Some(port) => format!("{h}:{port}"),
            None => h.clone(),
        };
        authority = Some(Rc::from(auth_canon.as_str()));
        host = Some(Rc::from(h.as_str()));
        path = if slash < after_slashes.len() {
            &after_slashes[slash..]
        } else {
            "/"
        };
    } else {
        path = without_query;
    }
    if path.contains('\0') {
        return Err("URL.parse: path contains NUL".to_string());
    }
    let query = parse_url_query(query_raw)?;
    let fragment = fragment_raw.map(Rc::from);
    let mut canonical = String::new();
    canonical.push_str(&scheme);
    canonical.push(':');
    if let Some(auth) = &authority {
        canonical.push_str("//");
        canonical.push_str(auth);
    }
    canonical.push_str(path);
    if let Some(q) = query_raw {
        canonical.push('?');
        canonical.push_str(q);
    }
    if let Some(f) = fragment_raw {
        canonical.push('#');
        canonical.push_str(f);
    }
    Ok(UrlData {
        canonical: Rc::from(canonical.as_str()),
        scheme: Rc::from(scheme.as_str()),
        authority,
        host,
        path: Rc::from(path),
        query,
        fragment,
    })
}

/// Host adapter seam for constructing the same canonical URL value as
/// `URL.parse` without routing through a Topaz callable. It performs no I/O.
pub fn url_value(text: &str) -> Result<Value, String> {
    parse_url_text(text).map(|url| Value::Url(Rc::new(url)))
}

pub(in crate::value) fn url_arg(
    arg: Value,
    name: &str,
    span: Span,
) -> Result<Rc<UrlData>, RtError> {
    match arg {
        Value::Url(url) => Ok(url),
        other => Err(fault(
            codes::GUARD_TYPE,
            format!("`URL.{name}` takes `URL`, found `{}`", other.kind()),
            span,
        )),
    }
}

pub fn builtin_url_parse(text: Value, span: Span) -> Result<Value, RtError> {
    let text = stdlib_string_arg(text, "URL", "parse", "text", span)?;
    Ok(match parse_url_text(&text) {
        Ok(url) => Value::Ok(Rc::new(Value::Url(Rc::new(url)))),
        Err(e) => err_string(e),
    })
}

pub fn builtin_url_scheme(url: Value, span: Span) -> Result<Value, RtError> {
    Ok(Value::Str(url_arg(url, "scheme", span)?.scheme.clone()))
}

pub fn builtin_url_host(url: Value, span: Span) -> Result<Value, RtError> {
    Ok(match &url_arg(url, "host", span)?.host {
        Some(host) => Value::Some(Rc::new(Value::Str(host.clone()))),
        None => Value::None,
    })
}

pub fn builtin_url_path(url: Value, span: Span) -> Result<Value, RtError> {
    Ok(Value::Str(url_arg(url, "path", span)?.path.clone()))
}

pub fn builtin_url_query(url: Value, span: Span) -> Result<Value, RtError> {
    let url = url_arg(url, "query", span)?;
    let mut map = OrderedMap::new();
    for (k, v) in url.query.iter() {
        let current = match map.get(&Key::Str(k.clone())) {
            Some(Value::Array(items)) => items.borrow().clone(),
            Some(_) => Vec::new(),
            None => Vec::new(),
        };
        let mut next = current;
        next.push(Value::Str(v.clone()));
        map.insert(Key::Str(k.clone()), Value::array(next));
    }
    Ok(Value::Map(Rc::new(RefCell::new(map))))
}

pub fn builtin_url_fragment(url: Value, span: Span) -> Result<Value, RtError> {
    Ok(match &url_arg(url, "fragment", span)?.fragment {
        Some(fragment) => Value::Some(Rc::new(Value::Str(fragment.clone()))),
        None => Value::None,
    })
}

pub fn builtin_url_to_string(url: Value, span: Span) -> Result<Value, RtError> {
    Ok(Value::Str(
        url_arg(url, "toString", span)?.canonical.clone(),
    ))
}
