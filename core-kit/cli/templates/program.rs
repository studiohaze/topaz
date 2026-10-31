//! Minimal host adapter for the native program emitted by Core Kit.
//! Command selection and Cargo inputs are authored in Topaz.

use std::rc::Rc;
use topaz_host_native::Host;
use topaz_rt::{RunOutcome, Value};

// Generated names and imports follow the canonical emitter's ABI. Keep its
// mechanical warnings out of user build logs without hiding adapter warnings.
#[allow(warnings)]
#[path = "generated.rs"]
mod program;

#[path = "capabilities.rs"]
mod capabilities;

fn run() -> i32 {
    // Use the host's cached snapshot so main's stdin argument and std.io's
    // readStdin see the same bytes. NativeHost does not read an interactive TTY.
    let host = match capabilities::host() {
        Ok(host) => Rc::new(host),
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    let stdin = host.input();
    let arguments = match std::env::args_os()
        .skip(1)
        .map(|arg| arg.into_string())
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(arguments) => arguments,
        Err(_) => {
            eprintln!("program arguments must be valid Unicode");
            return 1;
        }
    };
    let outcome = program::run_with_host_and_input(host, arguments, stdin);
    let code = match outcome {
        RunOutcome::Completed(_) if !program::TOPAZ_EXPLICIT_MAIN => 0,
        RunOutcome::Completed(Value::Ok(value)) => match value.as_ref() {
            Value::Int(code) if (0..=255).contains(code) => *code as i32,
            _ => {
                eprintln!("main must return Ok(exit_code) with an exit code in 0..255");
                1
            }
        },
        RunOutcome::Completed(Value::Err(value)) => {
            if let Value::Str(message) = value.as_ref() {
                eprintln!("{message}");
            } else {
                eprintln!("main returned a non-string error");
            }
            1
        }
        RunOutcome::Completed(_) => {
            eprintln!("main returned an invalid result");
            1
        }
        RunOutcome::Faulted(error) => {
            eprintln!("{error:?}");
            1
        }
    };
    code
}

fn main() {
    let result = std::thread::Builder::new()
        .name("Topaz program".into())
        .stack_size(topaz_product_runtime::PRODUCT_RUNTIME_STACK_BYTES)
        .spawn(run)
        .map_err(|error| format!("cannot start program: {error}"))
        .and_then(|thread| {
            thread.join().map_err(|_| "program thread panicked".to_string())
        });
    let code = match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    };
    std::process::exit(code);
}
