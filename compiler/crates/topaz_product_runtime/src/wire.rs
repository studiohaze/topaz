use crate::program::{decode_json::*, model::*};
use crate::runtime::{machine::Machine, model::*};
use crate::*;

pub(crate) fn run_on_self_runtime_stack<T, F>(name: &str, task: F) -> Result<T, String>
where
    T: Send,
    F: FnOnce() -> Result<T, String> + Send,
{
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name(name.to_string())
            .stack_size(PRODUCT_RUNTIME_STACK_BYTES)
            .spawn_scoped(scope, task)
            .map_err(|error| format!("cannot start {name}: {error}"))?
            .join()
            .map_err(|_| format!("{name} panicked"))?
    })
}
pub(crate) fn unit() -> RuntimeValue {
    RuntimeValue::Data(Value::Unit)
}

pub(crate) fn data(value: RuntimeValue) -> Result<Value, String> {
    match value {
        RuntimeValue::Data(value) => Ok(value),
        RuntimeValue::Function {
            operation,
            environment,
        } => Ok(product_closure(operation, environment)),
        RuntimeValue::Type(name) => Err(format!("expected data, found type `{name}`")),
        RuntimeValue::EnumConstructor {
            identity, variant, ..
        } => Err(format!(
            "expected data, found enum constructor `{identity}.{variant}`"
        )),
    }
}

pub(crate) fn span(operation: &Operation) -> Span {
    Span::new(FileId(0), operation.lo, operation.hi)
}

/// Decodes and runs a compiler-image payload for one exchange request.
pub fn execute_compiler(payload: &str, request_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let program = Arc::new(parse_program(payload, ProgramAdmission::CompilerImage)?.program);
    Machine::new(program).compile(request_bytes)
}

/// Runs an already admitted compiler program without host-fact injection.
pub fn execute_compiler_program(
    program: Arc<Program>,
    request_bytes: &[u8],
) -> Result<Vec<u8>, String> {
    Machine::new(program).compile(request_bytes)
}

/// Supplies compiler facts through the runtime host before invoking the exchange.
pub fn execute_compiler_program_with_facts(
    program: Arc<Program>,
    request_bytes: &[u8],
    compiler_facts: &str,
) -> Result<Vec<u8>, String> {
    Machine::new_with_facts_host_and_input(program, Some(compiler_facts), "", None)?
        .compile(request_bytes)
}

/// Execute one checked target program from the validated fixed-point IR table.
///
/// This is the shared runtime side of the dual-toolchain boundary: it consumes
/// compiler decisions already present in the table and never lexes, parses,
/// resolves, checks, or lowers target source. A single parameter on an explicit
/// `main` receives the ordinary array of command-line strings; zero parameters
/// receives no value. Other arities fail closed.
pub fn execute_product_program(
    payload: &str,
    program_args: &[String],
) -> Result<(Value, bool), String> {
    execute_product_program_with_facts_and_input(payload, program_args, "", None)
}

/// Execute one checked target program with its mechanically projected C2
/// nominal registry. The registry supplies declaration-order enum and record
/// facts which are intentionally absent from the fixed-point operation table.
pub fn execute_product_program_with_facts(
    payload: &str,
    program_args: &[String],
    target_facts: Option<&str>,
) -> Result<(Value, bool), String> {
    execute_product_program_with_facts_and_input(payload, program_args, "", target_facts)
}

/// Execute one checked target program with the host-provided stdin snapshot
/// and its mechanically projected C2 nominal registry.
///
/// `input()` reads only this invocation-local value. The fixed-point runtime
/// never discovers ambient stdin and cannot reuse input from an earlier
/// invocation.
pub fn execute_product_program_with_facts_and_input(
    payload: &str,
    program_args: &[String],
    stdin: &str,
    target_facts: Option<&str>,
) -> Result<(Value, bool), String> {
    #[cfg(target_arch = "wasm32")]
    {
        let execution = execute_product_program_wire(payload, program_args, stdin, target_facts)?;
        let value = canonical_abi_decode(&execution.0)
            .map_err(|error| format!("self product result is not canonical ABI data: {error}"))?;
        return Ok((value, execution.1));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let execution = run_on_self_runtime_stack("self product runtime", || {
            execute_product_program_wire(payload, program_args, stdin, target_facts)
        })?;
        let value = canonical_abi_decode(&execution.0)
            .map_err(|error| format!("self product result is not canonical ABI data: {error}"))?;
        Ok((value, execution.1))
    }
}

/// Execute a checked target program against the host selected by the ordinary
/// emitted-product harness.
///
/// Unlike the compiler-image runtime, target programs may contain admitted
/// host effects. The caller supplies the already capability-scoped host; this
/// runtime never constructs one or discovers ambient filesystem authority.
pub fn execute_product_program_with_host_facts_and_input(
    payload: &str,
    program_args: &[String],
    stdin: &str,
    target_facts: Option<&str>,
    host: Rc<dyn Host>,
) -> Result<(Value, bool), String> {
    let parsed = parse_program(payload, ProgramAdmission::TargetProduct)?;
    let program = Arc::new(parsed.program);
    if !parsed.requires_host {
        let execution = run_on_self_runtime_stack("self product runtime", move || {
            execute_parsed_product_program_wire_with_host(
                program,
                program_args,
                stdin,
                target_facts,
                None,
            )
        })?;
        let value = canonical_abi_decode(&execution.0)
            .map_err(|error| format!("self product result is not canonical ABI data: {error}"))?;
        return Ok((value, execution.1));
    }
    let execution = execute_parsed_product_program_wire_with_host(
        program,
        program_args,
        stdin,
        target_facts,
        Some(host),
    )?;
    let value = canonical_abi_decode(&execution.0)
        .map_err(|error| format!("self product result is not canonical ABI data: {error}"))?;
    Ok((value, execution.1))
}

fn execute_product_program_wire(
    payload: &str,
    program_args: &[String],
    stdin: &str,
    target_facts: Option<&str>,
) -> Result<(String, bool), String> {
    execute_product_program_wire_with_host(payload, program_args, stdin, target_facts, None)
}

fn execute_product_program_wire_with_host(
    payload: &str,
    program_args: &[String],
    stdin: &str,
    target_facts: Option<&str>,
    host: Option<Rc<dyn Host>>,
) -> Result<(String, bool), String> {
    let program = Arc::new(parse_program(payload, ProgramAdmission::TargetProduct)?.program);
    execute_parsed_product_program_wire_with_host(program, program_args, stdin, target_facts, host)
}

fn execute_parsed_product_program_wire_with_host(
    program: Arc<Program>,
    program_args: &[String],
    stdin: &str,
    target_facts: Option<&str>,
    host: Option<Rc<dyn Host>>,
) -> Result<(String, bool), String> {
    let mut machine = Machine::new_with_facts_host_and_input(program, target_facts, stdin, host)?;
    machine.register_functions()?;
    machine.initialize_modules()?;

    let entry_modules = machine
        .program
        .modules
        .iter()
        .filter(|module| module.entry)
        .collect::<Vec<_>>();
    if entry_modules.len() != 1 {
        return Err(format!(
            "self product runtime requires exactly one entry module, found {}",
            entry_modules.len()
        ));
    }
    let main_identity = format!("{}::main", entry_modules[0].identity);
    let Some(main) = machine.functions.get(&main_identity).copied() else {
        return Ok((canonical_abi_encode(&Value::Unit)?, false));
    };
    let parameter_count = machine.program.operations[main]
        .operands
        .iter()
        .filter(|operand| {
            matches!(
                machine.program.operations[**operand].kind.as_str(),
                "binding/parameter" | "binding/variadic-parameter"
            )
        })
        .count();
    let arguments = match parameter_count {
        0 => Vec::new(),
        1 => vec![RuntimeValue::Data(Value::array(
            program_args
                .iter()
                .map(|argument| Value::str(argument.clone()))
                .collect(),
        ))],
        2 => vec![
            RuntimeValue::Data(Value::array(
                program_args
                    .iter()
                    .map(|argument| Value::str(argument.clone()))
                    .collect(),
            )),
            RuntimeValue::Data(Value::str(machine.stdin.clone())),
        ],
        count => {
            return Err(format!(
                "self product runtime main expects {count} parameters; supported entry arities are zero, one, or two"
            ));
        }
    };
    let result = machine.call_function(main, arguments)?;
    Ok((canonical_abi_encode(&data(result)?)?, true))
}

/// Invokes one exported target function through the canonical value ABI.
pub fn execute_product_export(
    payload: &str,
    name: &str,
    arguments: Vec<Value>,
) -> Result<Value, String> {
    let arguments = canonical_abi_args_encode(&arguments)?;
    #[cfg(target_arch = "wasm32")]
    {
        let result = execute_product_export_wire(payload, name, &arguments)?;
        return canonical_abi_decode(&result).map_err(|error| {
            format!("self product export result is not canonical ABI data: {error}")
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let result = run_on_self_runtime_stack("self product export runtime", || {
            execute_product_export_wire(payload, name, &arguments)
        })?;
        canonical_abi_decode(&result).map_err(|error| {
            format!("self product export result is not canonical ABI data: {error}")
        })
    }
}

fn execute_product_export_wire(
    payload: &str,
    name: &str,
    arguments: &str,
) -> Result<String, String> {
    let arguments = canonical_abi_decode_args(arguments)?;
    canonical_abi_encode(&execute_product_export_in_place(payload, name, arguments)?)
}

/// Execute an exported target function without forcing host-only values through
/// the public canonical ABI. Native hosts use this path for values such as URL,
/// request, response, and file handles that are deliberately outside the public ABI.
/// It still consumes only the validated fixed-point table and invokes no
/// compiler phase.
pub fn execute_product_export_in_place(
    payload: &str,
    name: &str,
    arguments: Vec<Value>,
) -> Result<Value, String> {
    execute_product_export_in_place_with_facts(payload, name, arguments, None)
}

/// Execute an exported target function with C2 nominal declaration facts.
pub fn execute_product_export_in_place_with_facts(
    payload: &str,
    name: &str,
    arguments: Vec<Value>,
    target_facts: Option<&str>,
) -> Result<Value, String> {
    execute_product_export_in_place_with_host_facts(payload, name, arguments, target_facts, None)
}

/// Execute an exported target function with the capability-scoped emitted
/// product host. A missing host remains valid only for pure exports.
pub fn execute_product_export_in_place_with_host_facts(
    payload: &str,
    name: &str,
    arguments: Vec<Value>,
    target_facts: Option<&str>,
    host: Option<Rc<dyn Host>>,
) -> Result<Value, String> {
    let program = Arc::new(parse_program(payload, ProgramAdmission::TargetProduct)?.program);
    let mut machine = Machine::new_with_facts_host_and_input(program, target_facts, "", host)?;
    machine.register_functions()?;
    machine.initialize_modules()?;
    let entry_modules = machine
        .program
        .modules
        .iter()
        .filter(|module| module.entry)
        .collect::<Vec<_>>();
    if entry_modules.len() != 1 {
        return Err(format!(
            "self product runtime requires exactly one entry module, found {}",
            entry_modules.len()
        ));
    }
    let identity = format!("{}::{name}", entry_modules[0].identity);
    let function = machine
        .functions
        .get(&identity)
        .copied()
        .ok_or_else(|| format!("self product has no exported function `{name}`"))?;
    let arguments = arguments
        .into_iter()
        .map(RuntimeValue::Data)
        .collect::<Vec<_>>();
    data(machine.call_function(function, arguments)?)
}
