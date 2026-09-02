use num_bigint::BigInt;
use num_integer::Integer;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::thread;
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
const MAX_VALUE_DEPTH: usize = 256;

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

fn prepare_request(source: &[u8], limits: [u64; 4]) -> Result<Vec<u8>, String> {
    let mut request = PREPARE_MAGIC.to_vec();
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
    let mut request = EVALUATE_MAGIC.to_vec();
    push_field(&mut request, prepared)?;
    push_field(&mut request, input)?;
    request.extend(input_limit.to_be_bytes());
    for limit in limits {
        request.extend(limit.to_be_bytes());
    }
    Ok(request)
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

fn prepare(runtime: &Runtime, source: &str) -> Result<Outcome, String> {
    runtime.invoke(
        Operation::Prepare,
        &prepare_request(source.as_bytes(), [4096, 1_000_000, 1_000_000, 64])?,
    )
}

fn evaluate(
    runtime: &Runtime,
    prepared: &[u8],
    input: &[u8],
    limits: [u64; 9],
) -> Result<Outcome, String> {
    runtime.invoke(
        Operation::Evaluate,
        &evaluate_request(prepared, input, 4096, limits)?,
    )
}

fn generous_limits() -> [u64; 9] {
    [
        1_000_000, 1_000_000, 1_000, 256, 1_000_000, 1_000_000, 1_000_000, 100, 1_000_000,
    ]
}

fn run_evaluator_suite(runtime: Arc<Runtime>) -> Result<(String, u64), String> {
    let golden = prepare(&runtime, "(if (< 10 15) \"allow\" \"deny\")\n")?;
    if golden.category != Category::Prepared {
        return Err("golden-prepare".into());
    }
    let complete = evaluate(&runtime, &golden.payload, &[0], generous_limits())?;
    if complete.category != Category::Complete {
        return Err("golden-complete".into());
    }
    validate_value(&complete.payload, false)?;
    let result_sha256 = sha256(&complete.payload);
    let eval_work = complete.usage.ok_or("golden-usage")?[0];

    let list = encoded_list(&[encoded_integer("7"), vec![2]]);
    let list_rule = prepare(&runtime, "(car input)\n")?;
    let list_result = evaluate(&runtime, &list_rule.payload, &list, generous_limits())?;
    if list_result.category != Category::Complete {
        return Err("list-input".into());
    }
    validate_value(&list_result.payload, false)?;

    let record = encoded_record(&[("alpha", vec![2]), ("count", encoded_integer("7"))]);
    let record_rule = prepare(&runtime, "input\n")?;
    let record_result = evaluate(&runtime, &record_rule.payload, &record, generous_limits())?;
    if record_result.category != Category::Complete {
        return Err("record-input".into());
    }
    validate_value(&record_result.payload, false)?;

    let fault_rule = prepare(&runtime, "(car 1)\n")?;
    let fault = evaluate(&runtime, &fault_rule.payload, &[0], generous_limits())?;
    if fault.category != Category::SemanticFailure || !fault.payload.is_empty() {
        return Err("semantic-failure".into());
    }

    let closure_rule = prepare(&runtime, "(lambda (x) x)\n")?;
    let closure = evaluate(&runtime, &closure_rule.payload, &[0], generous_limits())?;
    if closure.category != Category::SemanticFailure || !closure.payload.is_empty() {
        return Err("nonportable-result".into());
    }

    let mut limited = generous_limits();
    limited[0] = eval_work.checked_sub(1).ok_or("eval-work-zero")?;
    let exhausted = evaluate(&runtime, &golden.payload, &[0], limited)?;
    if exhausted.category != Category::LimitExhaustion
        || exhausted.code != "eval_work"
        || !exhausted.payload.is_empty()
    {
        return Err("eval-work-minus-one".into());
    }

    let malformed = runtime.invoke(Operation::Prepare, b"wrong")?;
    if malformed.category != Category::RequestRefusal || !malformed.payload.is_empty() {
        return Err("malformed-prepare".into());
    }
    let bad_integer = [3, 0, 0, 0, 0, 0, 0, 0, 2, b'0', b'1'];
    let noncanonical = evaluate(
        &runtime,
        &record_rule.payload,
        &bad_integer,
        generous_limits(),
    )?;
    if noncanonical.category != Category::RequestRefusal || !noncanonical.payload.is_empty() {
        return Err("noncanonical-input".into());
    }
    let deferred = prepare(&runtime, "(expt 2 3)\n")?;
    if deferred.category != Category::RequestRefusal || !deferred.payload.is_empty() {
        return Err("deferred-capability".into());
    }

    let repeated = evaluate(&runtime, &golden.payload, &[0], generous_limits())?;
    if repeated != complete {
        return Err("sequential-state".into());
    }
    let mut handles = Vec::new();
    for _ in 0..4 {
        let runtime = Arc::clone(&runtime);
        let prepared = golden.payload.clone();
        handles.push(thread::spawn(move || {
            evaluate(&runtime, &prepared, &[0], generous_limits())
        }));
    }
    for handle in handles {
        let concurrent = handle.join().map_err(|_| "concurrent-panic")??;
        if concurrent != complete {
            return Err("concurrent-state".into());
        }
    }
    Ok((result_sha256, eval_work))
}

fn run_codec_suite() -> Result<(), String> {
    let positives = [
        vec![0],
        vec![1],
        vec![2],
        encoded_integer("-12"),
        encoded_rational("-3", "7"),
        {
            let mut value = vec![5];
            value.extend(1.5_f64.to_bits().to_be_bytes());
            value
        },
        {
            let mut value = vec![6];
            value.extend(('한' as u32).to_be_bytes());
            value
        },
        encoded_text(7, "символ"),
        encoded_text(8, "문자열"),
        encoded_list(&[encoded_integer("1"), vec![2]]),
        {
            let mut value = vec![10];
            value.extend(1_u64.to_be_bytes());
            value.extend(encoded_integer("1"));
            value.push(0);
            value
        },
        {
            let mut value = vec![11];
            value.extend(1_u64.to_be_bytes());
            value.push(1);
            value
        },
        {
            let mut value = vec![12];
            value.extend(3_u64.to_be_bytes());
            value.extend([1, 2, 3]);
            value
        },
        encoded_record(&[("a", vec![1]), ("한", encoded_integer("2"))]),
    ];
    for value in positives {
        validate_value(&value, true)?;
    }

    let mut deep = vec![9];
    deep.extend(1_u64.to_be_bytes());
    for _ in 0..MAX_VALUE_DEPTH {
        deep.push(9);
        deep.extend(1_u64.to_be_bytes());
    }
    deep.push(0);
    let negatives = [
        vec![255],
        vec![8, 0],
        vec![0, 0],
        encoded_integer("01"),
        encoded_integer("-0"),
        encoded_rational("1", "1"),
        encoded_rational("2", "4"),
        {
            let mut value = vec![5];
            value.extend(f64::INFINITY.to_bits().to_be_bytes());
            value
        },
        {
            let mut value = vec![6];
            value.extend(0xd800_u32.to_be_bytes());
            value
        },
        {
            let mut value = vec![8];
            value.extend(1_u64.to_be_bytes());
            value.push(0xff);
            value
        },
        encoded_record(&[("", vec![0])]),
        encoded_record_unsorted(),
        encoded_record_duplicate(),
        encoded_record(&[("a", vec![0])]),
        deep,
    ];
    for (index, value) in negatives.into_iter().enumerate() {
        let allow_record = index != 13;
        if validate_value(&value, allow_record).is_ok() {
            return Err(format!("negative-codec-{index}"));
        }
    }
    Ok(())
}

fn validate_value(bytes: &[u8], allow_host_record: bool) -> Result<(), String> {
    let mut cursor = ValueCursor::new(bytes);
    parse_value(&mut cursor, 1, allow_host_record)?;
    if cursor.offset != bytes.len() {
        return Err("value-trailing".into());
    }
    Ok(())
}

fn parse_value(
    cursor: &mut ValueCursor<'_>,
    depth: usize,
    allow_host_record: bool,
) -> Result<(), String> {
    if depth > MAX_VALUE_DEPTH {
        return Err("value-depth".into());
    }
    match cursor.byte()? {
        0..=2 => {}
        3 => {
            let text = cursor.field()?;
            if !valid_integer(text) {
                return Err("value-integer".into());
            }
        }
        4 => {
            let numerator = cursor.field()?;
            let denominator = cursor.field()?;
            if !valid_integer(numerator)
                || !valid_positive_integer(denominator)
                || denominator == b"1"
            {
                return Err("value-rational".into());
            }
            let numerator = BigInt::parse_bytes(numerator, 10).ok_or("value-rational")?;
            let denominator = BigInt::parse_bytes(denominator, 10).ok_or("value-rational")?;
            if numerator.gcd(&denominator) != BigInt::from(1_u8) {
                return Err("value-rational".into());
            }
        }
        5 => {
            if !f64::from_bits(cursor.u64()?).is_finite() {
                return Err("value-real".into());
            }
        }
        6 => {
            if char::from_u32(cursor.u32()?).is_none() {
                return Err("value-character".into());
            }
        }
        7 | 8 => {
            std::str::from_utf8(cursor.field()?).map_err(|_| "value-utf8")?;
        }
        9 | 11 => {
            let count = cursor.count()?;
            for _ in 0..count {
                parse_value(cursor, depth + 1, allow_host_record)?;
            }
        }
        10 => {
            let count = cursor.count()?;
            for _ in 0..count {
                parse_value(cursor, depth + 1, allow_host_record)?;
            }
            parse_value(cursor, depth + 1, allow_host_record)?;
        }
        12 => {
            let _ = cursor.field()?;
        }
        13 => {
            if !allow_host_record {
                return Err("value-host-record-result".into());
            }
            let count = cursor.count()?;
            let mut previous: Option<Vec<u8>> = None;
            for _ in 0..count {
                let key = cursor.field()?.to_vec();
                if key.is_empty() || std::str::from_utf8(&key).is_err() {
                    return Err("value-record-key".into());
                }
                if previous.as_ref().is_some_and(|value| value >= &key) {
                    return Err("value-record-order".into());
                }
                previous = Some(key);
                parse_value(cursor, depth + 1, allow_host_record)?;
            }
        }
        _ => return Err("value-tag".into()),
    }
    Ok(())
}

fn valid_integer(bytes: &[u8]) -> bool {
    match bytes {
        b"0" => true,
        [b'1'..=b'9', rest @ ..] => rest.iter().all(u8::is_ascii_digit),
        [b'-', b'1'..=b'9', rest @ ..] => rest.iter().all(u8::is_ascii_digit),
        _ => false,
    }
}

fn valid_positive_integer(bytes: &[u8]) -> bool {
    matches!(bytes, [b'1'..=b'9', rest @ ..] if rest.iter().all(u8::is_ascii_digit))
}

fn encoded_integer(text: &str) -> Vec<u8> {
    encoded_text(3, text)
}

fn encoded_rational(numerator: &str, denominator: &str) -> Vec<u8> {
    let mut value = encoded_text(4, numerator);
    value.extend((denominator.len() as u64).to_be_bytes());
    value.extend(denominator.as_bytes());
    value
}

fn encoded_text(tag: u8, text: &str) -> Vec<u8> {
    let mut value = vec![tag];
    value.extend((text.len() as u64).to_be_bytes());
    value.extend(text.as_bytes());
    value
}

fn encoded_list(items: &[Vec<u8>]) -> Vec<u8> {
    let mut value = vec![9];
    value.extend((items.len() as u64).to_be_bytes());
    for item in items {
        value.extend(item);
    }
    value
}

fn encoded_record(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut entries = entries.to_vec();
    entries.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let mut value = vec![13];
    value.extend((entries.len() as u64).to_be_bytes());
    for (key, item) in entries {
        value.extend((key.len() as u64).to_be_bytes());
        value.extend(key.as_bytes());
        value.extend(item);
    }
    value
}

fn encoded_record_unsorted() -> Vec<u8> {
    let mut value = vec![13];
    value.extend(2_u64.to_be_bytes());
    for key in ["b", "a"] {
        value.extend(1_u64.to_be_bytes());
        value.extend(key.as_bytes());
        value.push(0);
    }
    value
}

fn encoded_record_duplicate() -> Vec<u8> {
    let mut value = vec![13];
    value.extend(2_u64.to_be_bytes());
    for _ in 0..2 {
        value.extend(1_u64.to_be_bytes());
        value.push(b'a');
        value.push(0);
    }
    value
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
        let value = self.bytes.get(self.offset..end).ok_or("cursor-truncated")?;
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

struct ValueCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ValueCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self.offset.checked_add(length).ok_or("value-overflow")?;
        let value = self.bytes.get(self.offset..end).ok_or("value-truncated")?;
        self.offset = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().map_err(|_| "value-u32")?,
        ))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| "value-u64")?,
        ))
    }
    fn count(&mut self) -> Result<u64, String> {
        let count = self.u64()?;
        if count > (self.bytes.len() - self.offset) as u64 {
            return Err("value-count".into());
        }
        Ok(count)
    }
    fn field(&mut self) -> Result<&'a [u8], String> {
        let length = usize::try_from(self.u64()?).map_err(|_| "value-length")?;
        self.take(length)
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
    let result = (|| {
        run_codec_suite()?;
        let runtime = Arc::new(Runtime::new()?);
        let (result_sha256, eval_work) = run_evaluator_suite(runtime)?;
        Ok::<_, String>((result_sha256, eval_work))
    })();
    match result {
        Ok((result_sha256, eval_work)) => println!(
            "{{\"schema\":\"topaz.psh-c1-contract-suite/v1\",\"status\":\"passed\",\"codecPositiveKinds\":14,\"codecNegativeClasses\":15,\"evaluatorCases\":11,\"overlappingCalls\":4,\"admittedRuntimeCount\":1,\"portabilityEvidence\":\"none-single-admitted-runtime\",\"evaluatorSha256\":\"{EVALUATOR_SHA256}\",\"goldenResultSha256\":\"{result_sha256}\",\"goldenEvalWork\":\"{eval_work}\"}}"
        ),
        Err(error) => {
            eprintln!("Lispex contract suite failed: {error}");
            std::process::exit(1);
        }
    }
}
