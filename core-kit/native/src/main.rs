use std::rc::Rc;

use topaz_core_kit_native::{Capture, Compiler, Native, Process, repository_root, repository_root_text};
use topaz_host_native::NativeHost;
use topaz_rt::{RunOutcome, Value};

include!(concat!(env!("OUT_DIR"), "/generated_cli_module.rs"));

fn run() -> Result<i32, String> {
    let arguments = std::env::args_os().skip(1)
        .map(|value| value.into_string().map(Value::str)
            .map_err(|_| "Core Kit arguments must be UTF-8".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let root = repository_root()?;
    let root = repository_root_text(&root)?;
    // Do not read stdin here: help and normal commands must not wait for EOF.
    // The Topaz command may read input through the existing Host boundary.
    match generated_cli::call_export_with_host(
        Rc::new(NativeHost::new()),
        "run",
        vec![
            Value::array(arguments),
            Value::str(root),
            Value::Closure(Rc::new(Compiler)),
            Value::Closure(Rc::new(Process)),
            Value::Closure(Rc::new(Native)),
            Value::Closure(Rc::new(Capture)),
        ],
    ) {
        RunOutcome::Completed(Value::Ok(value)) => match value.as_ref() {
            Value::Int(code) => i32::try_from(*code)
                .map_err(|_| "CLI returned an exit code outside i32".into()),
            other => Err(format!("CLI returned a non-integer exit code: {other:?}")),
        },
        RunOutcome::Completed(Value::Err(error)) => match error.as_ref() {
            Value::Str(message) => Err(message.to_string()),
            other => Err(format!("CLI error: {other:?}")),
        },
        RunOutcome::Completed(other) => Err(format!("CLI returned an invalid result: {other:?}")),
        RunOutcome::Faulted(error) => Err(format!("CLI runtime fault: {error:?}")),
    }
}

fn main() {
    let result = std::thread::Builder::new()
        .name("Topaz Core Kit".into())
        .stack_size(topaz_product_runtime::PRODUCT_RUNTIME_STACK_BYTES)
        .spawn(run)
        .map_err(|error| format!("cannot start Core Kit: {error}"))
        .and_then(|thread| thread.join().map_err(|_| "Core Kit thread panicked".to_string()))
        .and_then(|result| result);
    match result {
        Ok(code) => std::process::exit(code),
        Err(message) => {
            eprintln!("topaz-core-kit: {message}");
            std::process::exit(1);
        }
    }
}
