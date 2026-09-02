use crate::*;

pub(crate) fn generated_ir_payload(generated_rust: &str) -> Result<&str, String> {
    const PREFIX: &str = "pub const TOPAZ_COMPILER_IR_JSON: &str = r";
    let start = generated_rust
        .find(PREFIX)
        .ok_or_else(|| "self compilation product omitted its fixed-point IR payload".to_string())?
        + PREFIX.len();
    let suffix = &generated_rust[start..];
    let hash_count = suffix.bytes().take_while(|byte| *byte == b'#').count();
    let quote = suffix
        .as_bytes()
        .get(hash_count)
        .copied()
        .ok_or_else(|| "self compilation product IR payload is truncated".to_string())?;
    if quote != b'"' {
        return Err(
            "self compilation product IR payload has an invalid raw-string opener".to_string(),
        );
    }
    let payload_start = start + hash_count + 1;
    let terminator = format!("\"{};", "#".repeat(hash_count));
    let relative_end = generated_rust[payload_start..]
        .find(&terminator)
        .ok_or_else(|| {
            "self compilation product IR payload has no canonical terminator".to_string()
        })?;
    Ok(&generated_rust[payload_start..payload_start + relative_end])
}

/// Borrows the admitted IR and projects target adapter facts from a completed C2 product.
pub fn project_self_target_runtime_inputs(
    product: &SelfCompilationProduct,
) -> Result<SelfTargetRuntimeInputs<'_>, String> {
    require_self_target_runtime_inputs(
        product,
        "self target runtime inputs require one completed C2 product",
    )
}

pub(crate) fn require_self_target_runtime_inputs<'a>(
    product: &'a SelfCompilationProduct,
    error: &str,
) -> Result<SelfTargetRuntimeInputs<'a>, String> {
    require_completed_self_compilation_product(product, error)?;
    project_self_target_runtime_inputs_from_completed(product)
}

pub(crate) fn project_self_target_runtime_inputs_from_completed(
    product: &SelfCompilationProduct,
) -> Result<SelfTargetRuntimeInputs<'_>, String> {
    Ok(SelfTargetRuntimeInputs {
        facts: project_target_adapter_facts_from_completed(product)?,
        ir_json: generated_ir_payload(product.generated_rust())?,
    })
}

/// Run a completed C2 product through the shared fixed-point IR runtime.
///
/// Validation happens before extraction, and the runtime consumes only the IR
/// bytes emitted by C2. It cannot invoke the Rust target front end or reuse a
/// previous result.
pub fn execute_self_compilation_product(
    product: &SelfCompilationProduct,
    program_args: &[String],
) -> Result<(Value, bool), String> {
    execute_self_compilation_product_with_input(product, program_args, "")
}

/// Run a completed C2 product with the invocation-local stdin snapshot.
///
/// The shared target runtime consumes this value for `input()` without
/// discovering ambient host state or invoking another compiler.
pub fn execute_self_compilation_product_with_input(
    product: &SelfCompilationProduct,
    program_args: &[String],
    stdin: &str,
) -> Result<(Value, bool), String> {
    let runtime_inputs = require_self_target_runtime_inputs(
        product,
        "rejected self compilation product cannot execute",
    )?;
    let target_facts = encode_target_adapter_facts(&runtime_inputs.facts);
    topaz_stage1_runtime::execute_product_program_with_facts_and_input(
        runtime_inputs.ir_json,
        program_args,
        stdin,
        Some(&target_facts),
    )
}

/// Run a completed C2 target product with the caller's capability-scoped host.
///
/// Compiler execution stays pure; only an already checked target product may
/// receive this ordinary runtime effect boundary.
pub fn execute_self_compilation_product_with_host_and_input(
    product: &SelfCompilationProduct,
    program_args: &[String],
    stdin: &str,
    host: Rc<dyn topaz_value::Host>,
) -> Result<(Value, bool), String> {
    let runtime_inputs = require_self_target_runtime_inputs(
        product,
        "rejected self compilation product cannot execute",
    )?;
    execute_self_target_runtime_inputs_with_host_and_input(
        runtime_inputs,
        program_args,
        stdin,
        host,
    )
}

/// Run already projected target inputs through the shared fixed-point runtime.
///
/// Callers that must admit invocation inputs from target facts can project once,
/// inspect those facts before effects begin, and move the same inputs here.
pub fn execute_self_target_runtime_inputs_with_host_and_input(
    runtime_inputs: SelfTargetRuntimeInputs<'_>,
    program_args: &[String],
    stdin: &str,
    host: Rc<dyn topaz_value::Host>,
) -> Result<(Value, bool), String> {
    let target_facts = encode_target_adapter_facts(&runtime_inputs.facts);
    topaz_stage1_runtime::execute_product_program_with_host_facts_and_input(
        runtime_inputs.ir_json,
        program_args,
        stdin,
        Some(&target_facts),
        host,
    )
}
