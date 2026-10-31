use std::io::{Read, Write};
use std::rc::Rc;
use topaz_host_none::{NoCapabilityHost, NoCapabilityLimits};
use topaz_rt::{CallFuture, JsonValue, RtCx, RunOutcome, TpzCall, Value};

mod seed_files;
use seed_files::read_bounded_utf8;

fn request_value(value: &JsonValue) -> Result<Value, String> {
    Ok(match value {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(value) => Value::Bool(*value),
        JsonValue::String(value) => Value::Str(value.clone()),
        JsonValue::Number(value) => Value::Int(
            value
                .int
                .ok_or("compiler request requires integral numbers")?,
        ),
        JsonValue::Array(values) => {
            Value::array(values.iter().map(request_value).collect::<Result<_, _>>()?)
        }
        JsonValue::Object(fields) => Value::Record(Rc::new(
            fields
                .iter()
                .map(|(name, value)| Ok((name.to_string(), request_value(value)?)))
                .collect::<Result<_, String>>()?,
        )),
    })
}

#[derive(Debug)]
struct ClosedFacts;
impl TpzCall for ClosedFacts {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn name(&self) -> Option<&str> {
        Some("compilerSourceFacts")
    }
    fn arity(&self) -> usize {
        1
    }
    fn param_name(&self, index: usize) -> Option<&str> {
        (index == 0).then_some("query")
    }
    fn call(&self, _context: RtCx, _arguments: Vec<Value>) -> CallFuture {
        let result = Value::Err(Rc::new(Value::str("compiler source facts are incomplete")));
        Box::pin(async move { Ok(result) })
    }
}

fn response_bytes(bytes: &[u8], rust_source: bool) -> Result<Vec<u8>, String> {
    if !rust_source {
        return Ok(bytes.to_vec());
    }
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    let value = topaz_rt::value::json_parse(text)
        .map_err(|error| format!("invalid compiler response: {error:?}"))?;
    let JsonValue::Object(fields) = value else {
        return Err("compiler response must be an object".into());
    };
    if !matches!(fields.get("status"), Some(JsonValue::String(status)) if status.as_ref() == "completed")
    {
        return Err("compiler did not complete the Rust source request".into());
    }
    match fields.get("generatedRust") {
        Some(JsonValue::String(source)) if !source.is_empty() => Ok(source.as_bytes().to_vec()),
        _ => Err("compiler response has no generated Rust".into()),
    }
}

fn bootstrap_request(root: &str) -> Result<Vec<u8>, String> {
    use std::path::{Component, Path};
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let input = read_bounded_utf8(&root.join("request.json"), 128 * 1024 * 1024)?;
    let mut value = topaz_rt::value::json_parse(&input)
        .map_err(|e| format!("invalid bootstrap request: {e:?}"))?;
    let JsonValue::Object(object) = &mut value else {
        return Err("bootstrap request must be an object".into());
    };
    let object = Rc::make_mut(object);
    let Some(JsonValue::Array(facts)) = object.get_mut("facts") else {
        return Err("bootstrap facts missing".into());
    };
    let mut total = 0;
    for fact in Rc::make_mut(facts) {
        let JsonValue::Object(fields) = fact else {
            return Err("invalid source fact".into());
        };
        let fields = Rc::make_mut(fields);
        if !matches!(fields.get("kind"), Some(JsonValue::String(s)) if s.as_ref() == "read-source")
        {
            continue;
        }
        let Some(JsonValue::String(logical)) = fields.get("logicalPath") else {
            return Err("source path missing".into());
        };
        let path = Path::new(logical.as_ref());
        if !path.components().all(|c| matches!(c, Component::Normal(_))) {
            return Err("invalid compiler source path".into());
        }
        let directory = if path.starts_with("std") {
            root.clone()
        } else {
            root.join("topaz")
        };
        let file = std::fs::canonicalize(directory.join(path))
            .map_err(|e| format!("cannot read {logical}: {e}"))?;
        if !file.starts_with(&directory) {
            return Err("source path escapes compiler directory".into());
        }
        let source = read_bounded_utf8(&file, 64 * 1024 * 1024 - total)?;
        total += source.len();
        fields.insert(Rc::from("source"), JsonValue::String(Rc::from(source)));
    }
    let mut output = String::new();
    topaz_rt::value::write_json_node(&mut output, &value);
    Ok(output.into_bytes())
}

fn run() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (rust_source, bootstrap) = match arguments.as_slice() {
        [] => (false, None),
        [argument] if argument == "--rust-source" => (true, None),
        [argument, root, output] if argument == "--bootstrap" => (true, Some((root, output))),
        _ => return Err("usage: topaz-seed [--rust-source] < request.json; topaz-seed --bootstrap <directory> <output>".into()),
    };
    let request = if let Some((root, _)) = bootstrap {
        bootstrap_request(root)?
    } else {
        let limit = 128 * 1024 * 1024;
        let mut request = Vec::new();
        std::io::stdin()
            .take(limit + 1)
            .read_to_end(&mut request)
            .map_err(|e| e.to_string())?;
        if request.len() as u64 > limit {
            return Err("compiler input exceeds byte limit".into());
        }
        request
    };
    let host = NoCapabilityHost::try_new("", NoCapabilityLimits::new(0, 0, 65536, 1))
        .map_err(|error| error.to_string())?;
    let text = std::str::from_utf8(&request).map_err(|e| e.to_string())?;
    let request = topaz_rt::value::json_parse(text)
        .map_err(|e| format!("invalid compiler request: {e:?}"))?;
    match topaz_seed_compiler::program::call_export_with_host(
        Rc::new(host),
        "compile",
        vec![
            request_value(&request)?,
            Value::Closure(Rc::new(ClosedFacts)),
            Value::Bool(false),
            Value::Bool(false),
        ],
    ) {
        RunOutcome::Completed(Value::Ok(value)) => {
            let response = topaz_rt::value::json_stringify(value.as_ref(), true)?;
            let bytes = response.as_bytes();
            if let Some((_, destination)) = bootstrap {
                let response = std::path::Path::new(destination).with_extension("response.json");
                std::fs::write(response, bytes)
                    .map_err(|e| format!("cannot save compiler response: {e}"))?;
            }
            let output = response_bytes(bytes, rust_source)?;
            if let Some((_, destination)) = bootstrap {
                std::fs::write(destination, output)
                    .map_err(|e| format!("cannot write regenerated Rust: {e}"))
            } else {
                std::io::stdout()
                    .write_all(&output)
                    .map_err(|e| format!("cannot write compiler response: {e}"))
            }
        }
        RunOutcome::Completed(Value::Err(error)) => {
            Err(format!("compiler rejected request: {error:?}"))
        }
        RunOutcome::Completed(_) => Err("compiler returned an invalid result".into()),
        RunOutcome::Faulted(error) => Err(format!("compiler runtime failed: {error:?}")),
    }
}

fn main() -> Result<(), String> {
    std::thread::Builder::new()
        .name("Topaz compiler seed".into())
        .stack_size(topaz_product_runtime::PRODUCT_RUNTIME_STACK_BYTES)
        .spawn(run)
        .map_err(|error| format!("cannot start compiler: {error}"))?
        .join()
        .map_err(|_| "compiler thread panicked".to_string())?
}
