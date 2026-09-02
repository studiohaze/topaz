use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use wasmtime::{Config, Engine, ExternType, Instance, Module, Store};

const EVALUATOR: &[u8] = include_bytes!(
    "../../../components/lispex-embed-evaluator/1.12.4/payload/lispex-embed-evaluator.wasm"
);
const EVALUATOR_SHA256: &str = "fa6e52559e1f5a43e50a3b7ac0cc5add6930cff0aed8aaff462cff4609362870";
const CORPUS_SCHEMA: &str = "topaz.psh-c3-heldout-corpus/v1";
const PREPARE_MAGIC: &[u8; 8] = b"LPXPRP01";
const EVALUATE_MAGIC: &[u8; 8] = b"LPXEVA01";
const RESPONSE_MAGIC: &[u8; 8] = b"LPXRSP01";
const SAFETY_FUEL: u64 = 1_000_000_000;
const MAX_RESPONSE: usize = 2 * 1024 * 1024;
const MAX_CASES: usize = 64;
const MAX_SOURCE: usize = 64 * 1024;
const MAX_INPUT: usize = 1024 * 1024;

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

impl Category {
    const fn label(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Complete => "complete",
            Self::SemanticFailure => "semantic-failure",
            Self::LimitExhaustion => "limit-exhaustion",
            Self::RequestRefusal => "request-refusal",
            Self::EngineFault => "engine-fault",
        }
    }
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

struct Case {
    id: String,
    source: String,
    input: Vec<u8>,
    category: String,
    code: String,
    payload: Vec<u8>,
}

struct Observation {
    id: String,
    category: String,
    code: String,
    payload_sha256: String,
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
    parse_response(&response)
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
    let mut cursor = Cursor::new(&bytes[10..]);
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
                && outcome.digests[5].is_none()
                && outcome.usage.is_some()
        }
        (Operation::Evaluate, Category::LimitExhaustion) => {
            outcome.payload.is_empty()
                && outcome.digests[..3].iter().all(Option::is_some)
                && outcome.digests[3..].iter().all(Option::is_none)
                && outcome.usage.is_some()
        }
        (Operation::Evaluate, Category::RequestRefusal) => {
            outcome.payload.is_empty()
                && outcome.digests[3..].iter().all(Option::is_none)
                && outcome.usage.is_none()
        }
        (Operation::Evaluate, Category::EngineFault) => {
            outcome.payload.is_empty() && outcome.usage.is_some()
        }
        _ => false,
    };
    valid
        .then_some(())
        .ok_or_else(|| "payload-atomicity".into())
}

fn push_field(output: &mut Vec<u8>, field: &[u8]) -> Result<(), String> {
    output.extend(
        u32::try_from(field.len())
            .map_err(|_| "field-length")?
            .to_be_bytes(),
    );
    output.extend(field);
    Ok(())
}

fn prepare_request(source: &[u8]) -> Result<Vec<u8>, String> {
    let mut request = PREPARE_MAGIC.to_vec();
    push_field(&mut request, source)?;
    for limit in [65_536_u64, 10_000_000, 10_000_000, 256] {
        request.extend(limit.to_be_bytes());
    }
    Ok(request)
}

fn evaluate_request(prepared: &[u8], input: &[u8]) -> Result<Vec<u8>, String> {
    let mut request = EVALUATE_MAGIC.to_vec();
    push_field(&mut request, prepared)?;
    push_field(&mut request, input)?;
    request.extend((MAX_INPUT as u64).to_be_bytes());
    for limit in [
        10_000_000_u64,
        10_000_000,
        10_000,
        256,
        1_000_000,
        1_000_000,
        1_000_000,
        1_000,
        1_000_000,
    ] {
        request.extend(limit.to_be_bytes());
    }
    Ok(request)
}

fn parse_cases(bytes: &[u8]) -> Result<Vec<Case>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "corpus-utf8")?;
    let mut lines = text.lines();
    if lines.next() != Some(CORPUS_SCHEMA) {
        return Err("corpus-schema".into());
    }
    let mut ids = BTreeSet::new();
    let mut cases = Vec::new();
    for line in lines {
        if line.is_empty() {
            return Err("corpus-empty-line".into());
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 6 {
            return Err("corpus-fields".into());
        }
        let id = fields[0];
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || !ids.insert(id.to_string())
        {
            return Err("corpus-id".into());
        }
        let source_bytes = decode_hex(fields[1])?;
        if source_bytes.is_empty() || source_bytes.len() > MAX_SOURCE {
            return Err("corpus-source-size".into());
        }
        let source = String::from_utf8(source_bytes).map_err(|_| "corpus-source-utf8")?;
        if !source.ends_with('\n') {
            return Err("corpus-source-newline".into());
        }
        let input = decode_hex(fields[2])?;
        if input.is_empty() || input.len() > MAX_INPUT {
            return Err("corpus-input-size".into());
        }
        if !matches!(fields[3], "complete" | "semantic-failure") {
            return Err("corpus-category".into());
        }
        if fields[4].is_empty() || !fields[4].is_ascii() {
            return Err("corpus-code".into());
        }
        cases.push(Case {
            id: id.to_string(),
            source,
            input,
            category: fields[3].to_string(),
            code: fields[4].to_string(),
            payload: decode_hex(fields[5])?,
        });
        if cases.len() > MAX_CASES {
            return Err("corpus-case-limit".into());
        }
    }
    if cases.is_empty() {
        return Err("corpus-empty".into());
    }
    Ok(cases)
}

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2)
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("corpus-hex".into());
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).map_err(|_| "corpus-hex")?;
            u8::from_str_radix(digits, 16).map_err(|_| "corpus-hex".into())
        })
        .collect()
}

fn run(runtime: &Runtime, cases: &[Case]) -> Result<Vec<Observation>, String> {
    let mut observations = Vec::with_capacity(cases.len());
    for case in cases {
        let prepared = runtime.invoke(
            Operation::Prepare,
            &prepare_request(case.source.as_bytes())?,
        )?;
        if prepared.category != Category::Prepared {
            return Err(format!(
                "case-{}-prepare-{}-{}",
                case.id,
                prepared.category.label(),
                prepared.code
            ));
        }
        let outcome = runtime.invoke(
            Operation::Evaluate,
            &evaluate_request(&prepared.payload, &case.input)?,
        )?;
        if outcome.category.label() != case.category
            || outcome.code != case.code
            || outcome.payload != case.payload
        {
            return Err(format!(
                "case-{}-mismatch-actual={}:{}:{}-expected={}:{}:{}",
                case.id,
                outcome.category.label(),
                outcome.code,
                sha256(&outcome.payload),
                case.category,
                case.code,
                sha256(&case.payload),
            ));
        }
        observations.push(Observation {
            id: case.id.clone(),
            category: outcome.category.label().to_string(),
            code: outcome.code,
            payload_sha256: sha256(&outcome.payload),
        });
    }
    Ok(observations)
}

fn json_escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if control <= '\u{001f}' => {
                output.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

fn result_json(corpus_sha256: &str, observations: &[Observation]) -> String {
    let cases = observations
        .iter()
        .map(|item| {
            format!(
                "    {{\"id\":{},\"category\":{},\"code\":{},\"payloadSha256\":\"sha256:{}\"}}",
                json_escape(&item.id),
                json_escape(&item.category),
                json_escape(&item.code),
                item.payload_sha256,
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        concat!(
            "{{\n",
            "  \"schema\": \"topaz.psh-c3-heldout-candidate-result/v1\",\n",
            "  \"status\": \"passed\",\n",
            "  \"candidateId\": \"topaz-lispex-private-embedding-candidate/c2r\",\n",
            "  \"evaluatorSha256\": \"sha256:{}\",\n",
            "  \"corpusSha256\": \"sha256:{}\",\n",
            "  \"caseCount\": {},\n",
            "  \"cases\": [\n{}\n  ],\n",
            "  \"fallbackCount\": 0,\n",
            "  \"externalWitness\": false\n",
            "}}\n"
        ),
        EVALUATOR_SHA256,
        corpus_sha256,
        observations.len(),
        cases,
    )
}

fn parse_arguments() -> Result<(PathBuf, Option<PathBuf>), String> {
    let mut corpus = None;
    let mut result = None;
    let mut arguments = env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--corpus") => {
                corpus = Some(PathBuf::from(arguments.next().ok_or("argument-corpus")?));
            }
            Some("--result") => {
                result = Some(PathBuf::from(arguments.next().ok_or("argument-result")?));
            }
            _ => return Err("argument-unknown".into()),
        }
    }
    Ok((corpus.ok_or("argument-corpus")?, result))
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self.offset.checked_add(length).ok_or("cursor-overflow")?;
        let value = self.offset..end;
        let bytes = self.bytes.get(value).ok_or("cursor-truncated")?;
        self.offset = end;
        Ok(bytes)
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

fn write_result(path: &Path, result: &str) -> Result<(), String> {
    fs::write(path, result).map_err(|_| "result-write".into())
}

fn main() {
    let result = (|| {
        let (corpus_path, result_path) = parse_arguments()?;
        let corpus = fs::read(&corpus_path).map_err(|_| "corpus-read")?;
        let corpus_sha256 = sha256(&corpus);
        let cases = parse_cases(&corpus)?;
        let runtime = Runtime::new()?;
        let observations = run(&runtime, &cases)?;
        let report = result_json(&corpus_sha256, &observations);
        if let Some(path) = result_path {
            write_result(&path, &report)?;
        }
        print!("{report}");
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        eprintln!("Lispex held-out candidate suite failed: {error}");
        std::process::exit(1);
    }
}
