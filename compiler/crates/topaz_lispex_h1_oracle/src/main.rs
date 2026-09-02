use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use wasmtime::{Config, Engine, ExternType, Instance, Module, Store};

const EVALUATOR: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.12.4/payload/lispex-embed-evaluator.wasm"
);
const EVALUATOR_SHA256: &str = "fa6e52559e1f5a43e50a3b7ac0cc5add6930cff0aed8aaff462cff4609362870";
const PREPARE_MAGIC: &[u8; 8] = b"LPXPRP01";
const EVALUATE_MAGIC: &[u8; 8] = b"LPXEVA01";
const RESPONSE_MAGIC: &[u8; 8] = b"LPXRSP01";
const SAFETY_FUEL: u64 = 1_000_000_000;
const MAX_RESPONSE: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Prepare,
    Evaluate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Prepared,
    Complete,
    SemanticFailure,
    LimitExhaustion,
    RequestRefusal,
    EngineFault,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Outcome {
    operation: Operation,
    category: Category,
    code: String,
    payload: Vec<u8>,
    digests: [Option<String>; 6],
    usage: Option<[u64; 9]>,
}

struct Runtime {
    engine: Engine,
    module: Module,
}

impl Runtime {
    fn new() -> Result<Self, String> {
        if sha256(EVALUATOR) != EVALUATOR_SHA256 {
            return Err("evaluator-digest".into());
        }
        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(false);
        let engine = Engine::new(&config).map_err(|_| "runtime-config")?;
        let module = Module::from_binary(&engine, EVALUATOR).map_err(|_| "runtime-module")?;
        if module.imports().next().is_some() {
            return Err("runtime-import".into());
        }
        let mut functions = Vec::new();
        let mut memory = None;
        for export in module.exports() {
            match export.ty() {
                ExternType::Func(_) => functions.push(export.name().to_string()),
                ExternType::Memory(ty) if export.name() == "memory" => {
                    memory = Some((ty.minimum(), ty.maximum()));
                }
                ExternType::Global(_) if matches!(export.name(), "__data_end" | "__heap_base") => {}
                _ => return Err("runtime-export".into()),
            }
        }
        functions.sort();
        let mut expected = [
            "lispex_embed_abi_version",
            "lispex_embed_alloc",
            "lispex_embed_dealloc",
            "lispex_embed_evaluate",
            "lispex_embed_prepare",
        ]
        .map(str::to_string)
        .to_vec();
        expected.sort();
        if functions != expected || memory != Some((18, Some(256))) {
            return Err("runtime-surface".into());
        }
        Ok(Self { engine, module })
    }

    fn invoke(&self, operation: Operation, request: &[u8]) -> Result<Outcome, String> {
        let mut store = Store::new(&self.engine, ());
        store.set_fuel(SAFETY_FUEL).map_err(|_| "safety-fuel")?;
        let instance = Instance::new(&mut store, &self.module, &[]).map_err(|_| "instantiate")?;
        invoke_instance(operation, request, instance, store)
    }
}

fn invoke_instance(
    operation: Operation,
    request: &[u8],
    instance: Instance,
    mut store: Store<()>,
) -> Result<Outcome, String> {
    let version = instance
        .get_typed_func::<(), u32>(&mut store, "lispex_embed_abi_version")
        .map_err(|_| "abi-export")?
        .call(&mut store, ())
        .map_err(|_| "abi-call")?;
    if version != 0x0001_0000 {
        return Err("abi-version".into());
    }
    let alloc = instance
        .get_typed_func::<u32, u32>(&mut store, "lispex_embed_alloc")
        .map_err(|_| "alloc-export")?;
    let dealloc = instance
        .get_typed_func::<(u32, u32), u32>(&mut store, "lispex_embed_dealloc")
        .map_err(|_| "dealloc-export")?;
    let call = instance
        .get_typed_func::<(u32, u32), u64>(
            &mut store,
            match operation {
                Operation::Prepare => "lispex_embed_prepare",
                Operation::Evaluate => "lispex_embed_evaluate",
            },
        )
        .map_err(|_| "operation-export")?;
    let memory = instance.get_memory(&mut store, "memory").ok_or("memory")?;
    let length = u32::try_from(request.len()).map_err(|_| "request-length")?;
    let pointer = alloc.call(&mut store, length).map_err(|_| "alloc-call")?;
    if pointer == 0 {
        return Err("alloc-zero".into());
    }
    memory
        .write(&mut store, pointer as usize, request)
        .map_err(|_| "request-write")?;
    let packed = call
        .call(&mut store, (pointer, length))
        .map_err(|_| "operation-call")?;
    if packed == 0 {
        return Err("response-zero".into());
    }
    let response_pointer = (packed >> 32) as u32;
    let response_length = packed as u32;
    if response_length as usize > MAX_RESPONSE {
        return Err("response-limit".into());
    }
    let mut response = vec![0; response_length as usize];
    memory
        .read(&store, response_pointer as usize, &mut response)
        .map_err(|_| "response-read")?;
    if dealloc
        .call(&mut store, (response_pointer, response_length))
        .map_err(|_| "dealloc-call")?
        != 1
    {
        return Err("dealloc-result".into());
    }
    let outcome = parse_response(&response)?;
    if outcome.operation != operation {
        return Err("response-operation-mismatch".into());
    }
    Ok(outcome)
}

fn parse_response(bytes: &[u8]) -> Result<Outcome, String> {
    if bytes.len() < 16 || bytes.get(..8) != Some(RESPONSE_MAGIC) {
        return Err("response-framing".into());
    }
    let operation = match bytes[8] {
        1 => Operation::Prepare,
        2 => Operation::Evaluate,
        _ => return Err("response-operation".into()),
    };
    let category = match (operation, bytes[9]) {
        (Operation::Prepare, 0) => Category::Prepared,
        (Operation::Evaluate, 1) => Category::Complete,
        (Operation::Evaluate, 2) => Category::SemanticFailure,
        (_, 3) => Category::LimitExhaustion,
        (_, 4) => Category::RequestRefusal,
        (_, 5) => Category::EngineFault,
        _ => return Err("response-category".into()),
    };
    let mut cursor = ByteCursor::new(&bytes[10..]);
    let code_length = cursor.u16()? as usize;
    let code = std::str::from_utf8(cursor.take(code_length)?)
        .map_err(|_| "response-code-utf8")?
        .to_string();
    if code.is_empty() || !code.is_ascii() {
        return Err("response-code".into());
    }
    let payload_length = cursor.u32()? as usize;
    let payload = cursor.take(payload_length)?.to_vec();
    let mut digests: [Option<String>; 6] = std::array::from_fn(|_| None);
    for digest in &mut digests {
        match cursor.byte()? {
            0 => {}
            1 => {
                let value = cursor.take(64)?;
                if !value
                    .iter()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                {
                    return Err("response-digest".into());
                }
                *digest = Some(
                    std::str::from_utf8(value)
                        .map_err(|_| "response-digest-utf8")?
                        .to_string(),
                );
            }
            _ => return Err("response-digest-tag".into()),
        }
    }
    let usage = match cursor.byte()? {
        0 => None,
        1 => {
            let mut usage = [0; 9];
            for value in &mut usage {
                *value = cursor.u64()?;
            }
            Some(usage)
        }
        _ => return Err("response-usage-tag".into()),
    };
    if !cursor.finished() {
        return Err("response-trailing".into());
    }
    let outcome = Outcome {
        operation,
        category,
        code,
        payload,
        digests,
        usage,
    };
    validate_atomicity(&outcome)?;
    Ok(outcome)
}

fn validate_atomicity(outcome: &Outcome) -> Result<(), String> {
    let valid = match (outcome.operation, outcome.category) {
        (Operation::Prepare, Category::Prepared) => {
            outcome.code == "prepared"
                && !outcome.payload.is_empty()
                && outcome.digests[..5].iter().all(Option::is_some)
                && outcome.digests[5].is_none()
                && outcome.usage.is_none()
        }
        (
            Operation::Prepare,
            Category::LimitExhaustion | Category::RequestRefusal | Category::EngineFault,
        ) => outcome.payload.is_empty() && outcome.usage.is_none(),
        (Operation::Evaluate, Category::Complete) => {
            outcome.code == "complete"
                && !outcome.payload.is_empty()
                && outcome.digests[..5].iter().all(Option::is_some)
                && outcome.digests[5].is_none()
                && outcome.usage.is_some()
        }
        (Operation::Evaluate, Category::SemanticFailure) => {
            outcome.payload.is_empty()
                && outcome.digests[..5].iter().all(Option::is_some)
                && outcome.digests[5].is_some()
                && outcome.usage.is_some()
        }
        (
            Operation::Evaluate,
            Category::LimitExhaustion | Category::RequestRefusal | Category::EngineFault,
        ) => outcome.payload.is_empty(),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err("response-atomicity".into())
    }
}

fn push_field(output: &mut Vec<u8>, value: &[u8]) -> Result<(), String> {
    let length = u32::try_from(value.len()).map_err(|_| "field-length")?;
    output.extend(length.to_be_bytes());
    output.extend(value);
    Ok(())
}

fn prepare_request(source: &[u8], limits: [u64; 4]) -> Result<Vec<u8>, String> {
    let mut request = Vec::with_capacity(12 + source.len() + 32);
    request.extend(PREPARE_MAGIC);
    push_field(&mut request, source)?;
    for limit in limits {
        request.extend(limit.to_be_bytes());
    }
    Ok(request)
}

fn evaluate_request(
    prepared: &[u8],
    input: &[u8],
    input_limit: u64,
    limits: [u64; 9],
) -> Result<Vec<u8>, String> {
    let mut request = Vec::with_capacity(16 + prepared.len() + input.len() + 80);
    request.extend(EVALUATE_MAGIC);
    push_field(&mut request, prepared)?;
    push_field(&mut request, input)?;
    request.extend(input_limit.to_be_bytes());
    for limit in limits {
        request.extend(limit.to_be_bytes());
    }
    Ok(request)
}

fn category_name(category: Category) -> &'static str {
    match category {
        Category::Prepared => "prepared",
        Category::Complete => "complete",
        Category::SemanticFailure => "semantic-failure",
        Category::LimitExhaustion => "limit-exhaustion",
        Category::RequestRefusal => "request-refusal",
        Category::EngineFault => "engine-fault",
    }
}

fn json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn option_strings(values: &[Option<String>; 6]) -> String {
    let values = values
        .iter()
        .map(|value| {
            value
                .as_deref()
                .map(json_string)
                .unwrap_or_else(|| "null".into())
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{values}]")
}

fn option_usage(value: Option<[u64; 9]>) -> String {
    match value {
        Some(values) => format!(
            "[{}]",
            values
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
        None => "null".into(),
    }
}

fn outcome_json(outcome: &Outcome, include_payload: bool) -> String {
    let payload_hex = if include_payload {
        json_string(&hex(&outcome.payload))
    } else {
        "null".into()
    };
    format!(
        "{{\"category\":{},\"code\":{},\"digests\":{},\"payloadHex\":{},\"payloadSha256\":{},\"usage\":{}}}",
        json_string(category_name(outcome.category)),
        json_string(&outcome.code),
        option_strings(&outcome.digests),
        payload_hex,
        json_string(&sha256(&outcome.payload)),
        option_usage(outcome.usage),
    )
}

fn parse_limit(value: &str, label: &str) -> Result<u64, String> {
    value.parse::<u64>().map_err(|_| format!("oracle-{label}"))
}

fn run(arguments: &[String]) -> Result<String, String> {
    if arguments.len() != 16 {
        return Err("oracle-arguments".into());
    }
    let source = fs::read(&arguments[0]).map_err(|_| "oracle-source-read")?;
    let input = fs::read(&arguments[1]).map_err(|_| "oracle-input-read")?;
    let prepare_limits = [
        parse_limit(&arguments[2], "prepare-raw-source-bytes")?,
        parse_limit(&arguments[3], "prepare-work")?,
        parse_limit(&arguments[4], "prepare-logical-allocation")?,
        parse_limit(&arguments[5], "prepare-syntax-depth")?,
    ];
    let mut evaluation_limits = [0_u64; 10];
    for (index, value) in evaluation_limits.iter_mut().enumerate() {
        *value = parse_limit(&arguments[index + 6], "evaluation-limit")?;
    }

    let runtime = Runtime::new()?;
    let preparation = runtime.invoke(
        Operation::Prepare,
        &prepare_request(&source, prepare_limits)?,
    )?;
    let evaluation = if preparation.category == Category::Prepared {
        Some(
            runtime.invoke(
                Operation::Evaluate,
                &evaluate_request(
                    &preparation.payload,
                    &input,
                    evaluation_limits[0],
                    evaluation_limits[1..]
                        .try_into()
                        .map_err(|_| "oracle-evaluation-limits")?,
                )?,
            )?,
        )
    } else {
        None
    };

    Ok(format!(
        "{{\"evaluation\":{},\"evaluatorSha256\":{},\"inputSha256\":{},\"preparation\":{},\"schema\":\"topaz.lda-h1.bounded-integration-oracle-result/v1\",\"sourceSha256\":{}}}",
        evaluation
            .as_ref()
            .map(|outcome| outcome_json(outcome, true))
            .unwrap_or_else(|| "null".into()),
        json_string(EVALUATOR_SHA256),
        json_string(&sha256(&input)),
        outcome_json(&preparation, false),
        json_string(&sha256(&source)),
    ))
}

struct ByteCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ByteCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self.offset.checked_add(length).ok_or("cursor-overflow")?;
        let value = self.offset..end;
        let value = self.bytes.get(value).ok_or("cursor-truncated")?;
        self.offset = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().map_err(|_| "cursor-u16")?,
        ))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().map_err(|_| "cursor-u32")?,
        ))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| "cursor-u64")?,
        ))
    }
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn main() {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match run(&arguments) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("LDA-H1 integration oracle failed: {error}");
            std::process::exit(1);
        }
    }
}
