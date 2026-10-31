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
            name,
        } => Ok(product_closure(operation, environment, name)),
        RuntimeValue::Type(name) => Err(format!("expected data, found type `{name}`")),
        RuntimeValue::Module(name) => Err(format!("expected data, found module `{name}`")),
        RuntimeValue::EnumConstructor {
            identity, variant, ..
        } => Err(format!(
            "expected data, found enum constructor `{identity}.{variant}`"
        )),
    }
}

pub(crate) fn span(operation: &Operation) -> Span {
    span_at(operation, operation.lo, operation.hi)
}

/// A span inside the operation's own source module.
pub(crate) fn span_at(operation: &Operation, lo: u32, hi: u32) -> Span {
    Span::new(FileId(operation.source_ordinal), lo, hi)
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

/// Immutable compiler facts shared between exchange requests.
#[derive(Clone)]
pub struct PreparedCompilerFacts(Arc<crate::runtime::nominal::NominalRegistry>);

pub fn prepare_compiler_facts(facts: &str) -> Result<PreparedCompilerFacts, String> {
    Ok(PreparedCompilerFacts(Arc::new(
        crate::runtime::nominal::NominalRegistry::parse(facts)?,
    )))
}

pub fn execute_compiler_program_with_prepared_facts(
    program: Arc<Program>,
    request_bytes: &[u8],
    facts: &PreparedCompilerFacts,
) -> Result<Vec<u8>, String> {
    let mut machine = Machine::new(program);
    machine.nominals = facts.0.clone();
    machine.compile(request_bytes)
}

/// Calls K's value entry on the caller's runtime thread. Arguments, callbacks
/// and the returned value never cross a thread boundary.
pub fn invoke_compiler_program_with_prepared_facts(
    program: Arc<Program>,
    arguments: Vec<Value>,
    facts: &PreparedCompilerFacts,
    host: Rc<dyn Host>,
) -> Result<Value, String> {
    let mut machine = Machine::new(program);
    machine.nominals = facts.0.clone();
    machine.host = Some(host);
    machine.register_functions()?;
    machine.initialize_modules()?;
    let entry = machine
        .functions
        .get("src.main::compile")
        .copied()
        .ok_or_else(|| "compiler value entry `compile` is missing".to_string())?;
    data(machine.call_function(
        entry,
        arguments.into_iter().map(RuntimeValue::Data).collect(),
    )?)
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

/// Execute one checked target program from the validated fixed-point IR table
/// with its mechanically projected C2 facts.
///
/// This is the shared runtime side of the dual-toolchain boundary: it consumes
/// compiler decisions already present in the table and the facts, and never
/// lexes, parses, resolves, checks, or lowers target source. Those facts carry
/// the entry-point judgement, so a program cannot be executed without them. A
/// single parameter on an explicit `main` receives the ordinary array of
/// command-line strings; zero parameters receives no value. Other arities fail
/// closed. The registry supplies declaration-order enum and record facts which
/// are intentionally absent from the fixed-point operation table.
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
/// `None` lets executed input calls use that host; `Some` supplies a fixed
/// snapshot, including an explicitly empty one. CLI callers supply main's
/// snapshot before entering the runtime.
pub fn execute_product_program_with_host_facts_and_input<'a>(
    payload: &str,
    program_args: &[String],
    stdin: impl Into<Option<&'a str>>,
    target_facts: Option<&str>,
    host: Rc<dyn Host>,
) -> Result<(Value, bool), String> {
    let stdin = stdin.into();
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

/// Evaluate module initialization and return the entry module's final value.
/// This entry does not call an exported `main`; the caller supplies all host
/// capabilities and invocation input, just as for ordinary product execution.
/// Values stay on the caller's runtime thread, including closures and handles
/// that have no canonical ABI encoding.
pub fn evaluate_product_program_with_host_facts_and_input(
    payload: &str,
    stdin: &str,
    target_facts: Option<&str>,
    host: Rc<dyn Host>,
) -> Result<Value, String> {
    let program = Arc::new(parse_program(payload, ProgramAdmission::TargetProduct)?.program);
    let entry_count = program.modules.iter().filter(|module| module.entry).count();
    if entry_count != 1 {
        return Err(format!(
            "self product evaluation requires exactly one entry module, found {entry_count}"
        ));
    }
    let mut machine =
        Machine::new_with_facts_host_and_input(program, target_facts, stdin, Some(host))?;
    machine.register_functions()?;
    data(machine.initialize_modules()?)
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
    execute_parsed_product_program_wire_with_host(
        program,
        program_args,
        Some(stdin),
        target_facts,
        host,
    )
}

fn execute_parsed_product_program_wire_with_host(
    program: Arc<Program>,
    program_args: &[String],
    stdin: Option<&str>,
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
    // The entry point is not judged here. `SelfTargetAdapterFacts::
    // has_explicit_main` decides it once, `explicitMain` carries it, and this
    // runtime only obeys it: a `main` that the entry module does not export is
    // an ordinary function and must not be invoked from the command line.
    let Some(explicit_main) = machine.explicit_main else {
        return Err(
            "self product runtime requires target adapter facts carrying `explicitMain`"
                .to_string(),
        );
    };
    if !explicit_main {
        return Ok((canonical_abi_encode(&Value::Unit)?, false));
    }
    let main_identity = format!("{}::main", entry_modules[0].identity);
    let main = machine
        .functions
        .get(&main_identity)
        .copied()
        .ok_or_else(|| {
            format!(
                "self target facts report an exported `main`, but the entry module has no `{main_identity}`"
            )
        })?;
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
            RuntimeValue::Data(Value::str(machine.input())),
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
