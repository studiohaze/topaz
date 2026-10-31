//! Materialize the fixed CLI request and invoke the unchanged public core.
//! This is bootstrap plumbing, not a project resolver or command implementation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use topaz_core_kit_native::{MAX_REQUEST_BYTES, compile_request, repository_root};
use topaz_rt::{JsonValue, json_parse, write_json_node};

type Fields = BTreeMap<Rc<str>, JsonValue>;

fn object<'a>(value: &'a JsonValue, label: &str) -> Result<&'a Fields, String> {
    match value {
        JsonValue::Object(fields) => Ok(fields),
        _ => Err(format!("{label} must be an object")),
    }
}

fn object_mut<'a>(value: &'a mut JsonValue, label: &str) -> Result<&'a mut Fields, String> {
    match value {
        JsonValue::Object(fields) => Ok(Rc::make_mut(fields)),
        _ => Err(format!("{label} must be an object")),
    }
}

fn field<'a>(fields: &'a Fields, name: &str) -> Result<&'a JsonValue, String> {
    fields.get(name).ok_or_else(|| format!("missing field `{name}`"))
}

fn string<'a>(fields: &'a Fields, name: &str) -> Result<&'a str, String> {
    match field(fields, name)? {
        JsonValue::String(value) => Ok(value),
        _ => Err(format!("field `{name}` must be a string")),
    }
}

fn array<'a>(fields: &'a Fields, name: &str) -> Result<&'a [JsonValue], String> {
    match field(fields, name)? {
        JsonValue::Array(values) => Ok(values),
        _ => Err(format!("field `{name}` must be an array")),
    }
}

fn read_text(path: &Path, limit: usize) -> Result<String, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if bytes.len() > limit {
        return Err(format!("{} exceeds its byte limit", path.display()));
    }
    String::from_utf8(bytes)
        .map_err(|error| format!("{} is not UTF-8: {error}", path.display()))
}

fn check_relative_path(relative: &str) -> Result<(), String> {
    if relative.len() > 4096 || relative.contains(['\\', ':'])
        || relative.bytes().any(|byte| byte.is_ascii_control())
        || relative.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(format!("CLI input must use a relative slash-delimited path without traversal: {relative}"));
    }
    Ok(())
}

fn named_entry(parent: &Path, name: &str) -> Result<fs::DirEntry, String> {
    let entries = fs::read_dir(parent).map_err(|error| format!("cannot list {}: {error}", parent.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot inspect {}: {error}", parent.display()))?;
        if entry.file_name() == name { return Ok(entry); }
    }
    Err(format!("CLI input is missing: {}", parent.join(name).display()))
}

fn repository_file(root: &Path, relative: &str) -> Result<PathBuf, String> {
    check_relative_path(relative)?;
    // Match kit build's component checks, including exact spelling on a
    // case-insensitive filesystem. A link inside the checkout is still a link.
    let mut path = root.to_path_buf();
    let mut parts = relative.split('/').peekable();
    while let Some(part) = parts.next() {
        let entry = named_entry(&path, part)?;
        let kind = entry.file_type().map_err(|error| format!("cannot inspect {relative}: {error}"))?;
        let valid = if parts.peek().is_some() { kind.is_dir() } else { kind.is_file() };
        if !valid {
            return Err(format!("CLI input must use regular directories and files: {relative}"));
        }
        path = entry.path();
    }
    let path = path.canonicalize()
        .map_err(|error| format!("cannot resolve {relative}: {error}"))?;
    if !path.starts_with(root) || !path.is_file() {
        return Err(format!("input is not a file inside the checkout: {relative}"));
    }
    Ok(path)
}

fn parse_file(path: &Path) -> Result<JsonValue, String> {
    json_parse(&read_text(path, MAX_REQUEST_BYTES)?)
        .map_err(|error| format!("invalid JSON in {}: {error:?}", path.display()))
}

fn prepare_request(root: &Path) -> Result<String, String> {
    let descriptor = parse_file(&repository_file(root, "core-kit/cli/REQUEST.json")?)?;
    let descriptor = object(&descriptor, "CLI descriptor")?;
    if string(descriptor, "schema")? != "topaz.note.core-kit-cli-request/v2" {
        return Err("unsupported CLI request descriptor schema".into());
    }
    let request_path = repository_file(root, string(descriptor, "requestFile")?)?;
    let mut request = parse_file(&request_path)?;
    let request_fields = object_mut(&mut request, "CLI request")?;
    if string(request_fields, "schema")? != "topaz.compiler.stage1-exchange/v2"
        || string(request_fields, "terminal")? != "rust-source"
        || string(request_fields, "producer")? != "topaz-stage1"
        || string(request_fields, "languageMode")? != "topaz-5.21"
    {
        return Err("CLI input must be a Topaz 5.21 Stage 1 rust-source request".into());
    }
    let manifest = parse_file(&repository_file(root, "compiler/generated/native/compiler.json")?)?;
    let manifest = object(&manifest, "canonical compiler manifest")?;
    if string(request_fields, "compilerSourceSetId")? != string(manifest, "sourceSetId")?
        || string(descriptor, "compilerSourceSetId")? != string(manifest, "sourceSetId")?
    {
        return Err("CLI input does not identify the checked-in canonical compiler".into());
    }

    let mut sources = BTreeMap::new();
    for mapping in array(descriptor, "sourceFiles")? {
        let mapping = object(mapping, "source mapping")?;
        let logical = string(mapping, "logicalPath")?;
        check_relative_path(logical)?;
        let path = repository_file(root, string(mapping, "path")?)?;
        if sources.insert(logical.to_string(), path).is_some() {
            return Err(format!("duplicate CLI source mapping: {logical}"));
        }
    }
    if sources.is_empty() {
        return Err("CLI source mapping must not be empty".into());
    }
    let entry = string(request_fields, "entry")?.to_string();
    if !sources.contains_key(&entry) {
        return Err("CLI entry has no explicit source mapping".into());
    }
    let Some(JsonValue::Array(facts)) = request_fields.get_mut("facts") else {
        return Err("CLI source facts are missing".into());
    };
    let mut fact_keys = BTreeSet::new();
    let mut source_bytes = 0;
    for fact in Rc::make_mut(facts) {
        let fact = object_mut(fact, "CLI source fact")?;
        let kind = string(fact, "kind")?;
        let mount = string(fact, "mountId")?;
        let logical = string(fact, "logicalPath")?;
        if !fact_keys.insert((kind.to_string(), mount.to_string(), logical.to_string())) {
            return Err(format!("duplicate CLI source fact: {kind} {mount} {logical}"));
        }
        if kind != "read-source" {
            continue;
        }
        if mount != "root" {
            return Err("initial CLI sources must use the root mount".into());
        }
        if string(fact, "status")? != "present" {
            return Err("initial CLI read-source facts must be present".into());
        }
        let path = sources.remove(logical)
            .ok_or_else(|| format!("CLI fact has no explicit input mapping: {logical}"))?;
        let source = read_text(&path, 64 * 1024 * 1024 - source_bytes)?;
        source_bytes += source.len();
        fact.insert(Rc::from("source"), JsonValue::String(Rc::from(source)));
    }
    if !sources.is_empty() {
        return Err("CLI source mapping contains a file with no read-source fact".into());
    }

    let mut standards = BTreeMap::new();
    let mut standard_paths = BTreeSet::new();
    for mapping in array(descriptor, "standardModules")? {
        let mapping = object(mapping, "standard module mapping")?;
        let identity = string(mapping, "identity")?;
        let logical = string(mapping, "path")?.to_string();
        check_relative_path(&logical)?;
        if !standard_paths.insert(logical.clone()) {
            return Err(format!("duplicate CLI standard module path: {logical}"));
        }
        let path = repository_file(root, string(mapping, "sourceFile")?)?;
        if standards.insert(identity.to_string(), (logical, path)).is_some() {
            return Err(format!("duplicate CLI standard module mapping: {identity}"));
        }
    }
    let package = request_fields.get_mut("package").ok_or("CLI package facts are missing")?;
    let package = object_mut(package, "CLI package facts")?;
    let Some(JsonValue::Array(modules)) = package.get_mut("generatedStdModules") else {
        return Err("CLI generated standard modules are missing".into());
    };
    let mut used = BTreeSet::new();
    for module in Rc::make_mut(modules) {
        let module = object_mut(module, "CLI standard module")?;
        let identity = string(module, "identity")?.to_string();
        if !used.insert(identity.clone()) {
            return Err(format!("duplicate CLI generated standard module: {identity}"));
        }
        let (logical, path) = standards.get(&identity)
            .ok_or_else(|| format!("CLI standard module has no explicit mapping: {identity}"))?;
        if string(module, "path")? != logical {
            return Err(format!("CLI standard module path differs: {identity}"));
        }
        let source = read_text(path, 64 * 1024 * 1024 - source_bytes)?;
        source_bytes += source.len();
        module.insert(Rc::from("source"), JsonValue::String(Rc::from(source)));
    }
    if used.len() != standards.len() {
        return Err("CLI standard mapping contains an unused module".into());
    }
    let mut output = String::new();
    write_json_node(&mut output, &request);
    output.push('\n');
    Ok(output)
}

fn run() -> Result<(), String> {
    let root = repository_root()?;
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let output = match arguments.as_slice() {
        [] => root.join("out/core-kit-bootstrap"),
        [directory] => PathBuf::from(directory),
        _ => return Err("usage: topaz-core-kit-generate-cli [output-directory]".into()),
    };
    let request = prepare_request(&root)?;
    fs::create_dir_all(&output)
        .map_err(|error| format!("cannot create {}: {error}", output.display()))?;
    fs::write(output.join("cli.request.json"), &request)
        .map_err(|error| format!("cannot save prepared CLI request: {error}"))?;
    let response = compile_request(&request)?;
    let response_path = output.join("cli.response.json");
    fs::write(&response_path, &response)
        .map_err(|error| format!("cannot save CLI compiler response: {error}"))?;
    let response = json_parse(&response)
        .map_err(|error| format!("invalid compiler response: {error:?}"))?;
    let response = object(&response, "CLI compiler response")?;
    if string(response, "status")? != "completed"
        || string(response, "kind")? != "rust-source"
    {
        return Err(format!("CLI compilation did not complete; inspect {}", response_path.display()));
    }
    let rust = string(response, "generatedRust")?;
    if rust.is_empty() {
        return Err("CLI compiler returned empty Rust output".into());
    }
    let destination = output.join("generated_cli.rs");
    fs::write(&destination, rust)
        .map_err(|error| format!("cannot write generated CLI Rust: {error}"))?;
    println!("generated CLI Rust: {}", destination.display());
    Ok(())
}

fn main() {
    let result = std::thread::Builder::new()
        .name("Topaz Core Kit bootstrap".into())
        .stack_size(topaz_product_runtime::PRODUCT_RUNTIME_STACK_BYTES)
        .spawn(run)
        .map_err(|error| format!("cannot start CLI generation: {error}"))
        .and_then(|thread| thread.join().map_err(|_| "CLI generation thread panicked".to_string()))
        .and_then(|result| result);
    if let Err(message) = result {
        eprintln!("topaz-core-kit-generate-cli: {message}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_paths_require_exact_portable_components_and_regular_files() {
        let root = std::env::temp_dir().join(format!("topaz-input-paths-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let sources = root.join("src");
        fs::create_dir(&sources).unwrap();
        fs::write(sources.join("main.tpz"), b"source").unwrap();
        assert_eq!(repository_file(&root, "src/main.tpz").unwrap(), sources.join("main.tpz"));
        for path in [
            "", "src", "src/Main.tpz", "src/missing.tpz", "src/main.tpz/",
            "/src/main.tpz", "C:/src/main.tpz", "src\\main.tpz", "src//main.tpz",
            "src/./main.tpz", "src/../src/main.tpz", "src/main\t.tpz",
        ] {
            assert!(repository_file(&root, path).is_err(), "accepted {path:?}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&sources, root.join("alias")).unwrap();
            std::os::unix::fs::symlink(sources.join("main.tpz"), root.join("alias.tpz")).unwrap();
            assert!(repository_file(&root, "alias/main.tpz").is_err());
            assert!(repository_file(&root, "alias.tpz").is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
