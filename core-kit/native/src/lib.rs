//! The Core Kit ABI boundary. Commands, requests and build policy live in Topaz.
//! This crate only transports values to the unchanged canonical core and the OS.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use topaz_host_none::{NoCapabilityHost, NoCapabilityLimits};
use topaz_rt::{CallFuture, JsonValue, RtCx, RunOutcome, TpzCall, Value};

mod platform;
mod process;
pub use platform::Native;
pub use process::{Capture, Process};

pub const MAX_REQUEST_BYTES: usize = 128 * 1024 * 1024;

fn request_value(value: &JsonValue) -> Result<Value, String> {
    Ok(match value {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(value) => Value::Bool(*value),
        JsonValue::String(value) => Value::Str(value.clone()),
        JsonValue::Number(value) => Value::Int(
            value.int.ok_or("compiler requests require integral numbers")?,
        ),
        JsonValue::Array(values) => {
            Value::array(values.iter().map(request_value).collect::<Result<_, _>>()?)
        }
        JsonValue::Object(fields) => Value::record(
            fields
                .iter()
                .map(|(name, value)| Ok((name.to_string(), request_value(value)?)))
                .collect::<Result<Vec<_>, String>>()?,
        ),
    })
}

#[derive(Debug)]
struct ClosedFacts;

impl TpzCall for ClosedFacts {
    fn as_any(&self) -> &dyn std::any::Any { self }
    fn name(&self) -> Option<&str> { Some("closedSourceFacts") }
    fn arity(&self) -> usize { 1 }
    fn param_name(&self, index: usize) -> Option<&str> {
        (index == 0).then_some("queries")
    }
    fn call(&self, _context: RtCx, _arguments: Vec<Value>) -> CallFuture {
        let result = Value::Err(Rc::new(Value::str(
            "source facts are incomplete; provide a closed request containing every imported module",
        )));
        Box::pin(async move { Ok(result) })
    }
}

/// Invoke exactly the seed's canonical compile export, with no reference or
/// website compiler fallback. Compiler diagnostics remain in the response.
pub fn compile_request(text: &str) -> Result<String, String> {
    compile_with_provider(text, Rc::new(ClosedFacts))
}

fn compile_with_provider(text: &str, provider: Rc<dyn TpzCall>) -> Result<String, String> {
    if text.len() > MAX_REQUEST_BYTES {
        return Err("compiler request exceeds 128 MiB".into());
    }
    let request = topaz_rt::json_parse(text)
        .map_err(|error| format!("invalid compiler request JSON: {error:?}"))?;
    let host = NoCapabilityHost::try_new("", NoCapabilityLimits::new(0, 0, 65536, 1))
        .map_err(|error| error.to_string())?;
    match topaz_seed_compiler::program::call_export_with_host(
        Rc::new(host),
        "compile",
        vec![
            request_value(&request)?,
            Value::Closure(provider),
            Value::Bool(false),
            Value::Bool(false),
        ],
    ) {
        RunOutcome::Completed(Value::Ok(value)) => topaz_rt::json_stringify(&value, true),
        RunOutcome::Completed(Value::Err(error)) => match error.as_ref() {
            Value::Str(message) => Err(message.to_string()),
            other => Err(format!("canonical compiler rejected the request: {other:?}")),
        },
        RunOutcome::Completed(other) => Err(format!("invalid compiler result: {other:?}")),
        RunOutcome::Faulted(error) => Err(format!("canonical compiler fault: {error:?}")),
    }
}

// The core runs without filesystem capabilities. Its fact callback returns to
// the original CLI context, where the Topaz provider owns project I/O policy.
struct SourceProvider {
    callback: Rc<dyn TpzCall>,
    context: RtCx,
}

impl std::fmt::Debug for SourceProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceProvider").finish_non_exhaustive()
    }
}

impl TpzCall for SourceProvider {
    fn as_any(&self) -> &dyn std::any::Any { self }
    fn name(&self) -> Option<&str> { Some("sourceProvider") }
    fn arity(&self) -> usize { 1 }
    fn param_name(&self, index: usize) -> Option<&str> {
        (index == 0).then_some("query")
    }
    fn call(&self, _core_context: RtCx, arguments: Vec<Value>) -> CallFuture {
        let callback = self.callback.clone();
        let context = self.context.clone();
        let query = match arguments.as_slice() {
            [query] => topaz_rt::json_stringify(query, false),
            _ => Err("source provider expects one query".into()),
        };
        Box::pin(async move {
            let text = match query {
                Ok(text) => text,
                Err(error) => return Ok(result_value(Err(error))),
            };
            let result = callback.call(context, vec![Value::str(text)]).await?;
            let result = match result {
                Value::Ok(value) => match value.as_ref() {
                    Value::Str(text) if text.len() <= MAX_REQUEST_BYTES => {
                        topaz_rt::json_parse(text)
                            .map_err(|error| format!("invalid source fact JSON: {error:?}"))
                            .and_then(|fact| request_value(&fact))
                    }
                    Value::Str(_) => Err("source fact exceeds 128 MiB".into()),
                    _ => Err("source provider must return a JSON string".into()),
                },
                Value::Err(error) => match error.as_ref() {
                    Value::Str(message) => Err(message.to_string()),
                    _ => Err("source provider returned a non-string error".into()),
                },
                _ => Err("source provider must return Result<string, string>".into()),
            };
            Ok(result_value(result))
        })
    }
}

fn result_value(result: Result<Value, String>) -> Value {
    match result {
        Ok(value) => Value::Ok(Rc::new(value)),
        Err(message) => Value::Err(Rc::new(Value::str(message))),
    }
}

#[derive(Debug)]
pub struct Compiler;

impl TpzCall for Compiler {
    fn as_any(&self) -> &dyn std::any::Any { self }
    fn name(&self) -> Option<&str> { Some("compile") }
    fn arity(&self) -> usize { 2 }
    fn param_name(&self, index: usize) -> Option<&str> {
        ["request", "provide"].get(index).copied()
    }
    fn call(&self, context: RtCx, arguments: Vec<Value>) -> CallFuture {
        let result = match arguments.as_slice() {
            [Value::Str(text), Value::Closure(callback)] if callback.arity() == 1 => {
                compile_with_provider(text, Rc::new(SourceProvider {
                    callback: callback.clone(), context,
                })).map(Value::str)
            }
            _ => Err("compile expects a JSON request and a one-argument source provider".into()),
        };
        Box::pin(async move { Ok(result_value(result)) })
    }
}

fn is_repository(root: &Path) -> bool {
    root.join("core-kit/cli/REQUEST.json").is_file()
        && root.join("compiler/generated/native/compiler.json").is_file()
}

/// Find data relative to the actual executable, so a checked-in dist directory
/// works after clone/relocation. No build-machine path is embedded in the ABI.
pub fn repository_root() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os("TOPAZ_CORE_KIT_ROOT") {
        let root = PathBuf::from(root).canonicalize()
            .map_err(|error| format!("invalid TOPAZ_CORE_KIT_ROOT: {error}"))?;
        return if is_repository(&root) {
            Ok(root)
        } else {
            Err("TOPAZ_CORE_KIT_ROOT does not identify a Core Kit checkout".into())
        };
    }
    let executable = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|error| format!("cannot locate Core Kit executable: {error}"))?;
    executable.ancestors().skip(1).find(|path| is_repository(path))
        .map(Path::to_path_buf)
        .ok_or_else(|| "cannot locate Core Kit data; keep the executable inside its checkout or set TOPAZ_CORE_KIT_ROOT".into())
}

/// Project the OS path into the slash-delimited form used by Topaz and Cargo
/// manifests. Windows canonicalize returns an extended path that cannot accept
/// slash-delimited suffixes; preserve its drive/UNC identity without that prefix.
pub fn repository_root_text(root: &Path) -> Result<String, String> {
    let text = root.to_str().ok_or("Core Kit repository path must be UTF-8")?;
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        match root.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(_) => {
                    let path = text.strip_prefix(r"\\?\")
                        .ok_or("invalid extended Windows drive path")?;
                    Ok(path.replace('\\', "/"))
                }
                Prefix::VerbatimUNC(_, _) => {
                    let path = text.strip_prefix(r"\\?\UNC\")
                        .ok_or("invalid extended Windows UNC path")?;
                    Ok(format!("//{}", path.replace('\\', "/")))
                }
                Prefix::Verbatim(_) | Prefix::DeviceNS(_) => {
                    Err("Core Kit requires a drive or UNC repository path".into())
                }
                _ => Ok(text.replace('\\', "/")),
            },
            _ => Ok(text.replace('\\', "/")),
        }
    }
    #[cfg(not(windows))]
    {
        Ok(text.to_string())
    }
}
