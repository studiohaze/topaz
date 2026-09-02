use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;

use topaz_value::value::{JsonValue, json_parse, write_json_node};

fn topaz() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_topaz"));
    command.args(["--compiler", "rust"]);
    command
}

fn compiler_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn source() -> PathBuf {
    compiler_root().join("lit/lit.tpz")
}

fn fixture(path: &str) -> Vec<u8> {
    std::fs::read(compiler_root().join("lit/j57-2c/fixtures").join(path)).unwrap()
}

fn authority() -> JsonValue {
    json_parse(
        &std::fs::read_to_string(
            compiler_root().join("lit/j57-2c/diagnostic-pair-authority.v1.json"),
        )
        .unwrap(),
    )
    .unwrap()
}

fn run_lit(input: &[u8]) -> std::process::Output {
    let mut child = topaz()
        .arg("run")
        .arg(source())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Topaz interpreter starts");
    child
        .stdin
        .as_mut()
        .expect("interpreter stdin is piped")
        .write_all(input)
        .expect("request bytes are written");
    child.wait_with_output().expect("interpreter completes")
}

fn run_command_with_input(mut command: Command, input: &[u8]) -> std::process::Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("child process starts");
    child
        .stdin
        .as_mut()
        .expect("child stdin is piped")
        .write_all(input)
        .expect("child input bytes are written");
    child.wait_with_output().expect("child process completes")
}

fn run_historical_interpreter_adapter(input: &[u8]) -> std::process::Output {
    let mut command = Command::new("node");
    command
        .arg(compiler_root().join("lit/j57-2c/adapters/host-adapter.mjs"))
        .arg("--host")
        .arg("topaz-interpreter")
        .arg("--artifact-path")
        .arg("evidence/topaz-interpreter/lit.tpz")
        .arg("--topaz")
        .arg(env!("CARGO_BIN_EXE_topaz"))
        .arg("--source")
        .arg(source())
        .current_dir(compiler_root().parent().unwrap());
    run_command_with_input(command, input)
}

fn v1_result_with_raw(input: &[u8]) -> (JsonValue, String) {
    let output = run_lit(input);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(output.stdout.ends_with(b"\n"), "{output:?}");
    assert!(
        !output.stdout[..output.stdout.len() - 1].contains(&b'\n'),
        "{output:?}"
    );
    let raw = String::from_utf8(output.stdout).unwrap();
    let value = json_parse(&raw).expect("v1 result is one JSON object");
    (value, raw)
}

fn v1_result(input: &[u8]) -> JsonValue {
    v1_result_with_raw(input).0
}

fn v1_batch_raw(requests: &[Vec<u8>]) -> String {
    let mut input = String::from("[\"topaz.lit-j57-2c-diagnostic-projection-batch/v1\",[");
    for (request_index, request) in requests.iter().enumerate() {
        if request_index > 0 {
            input.push(',');
        }
        let request_text = String::from_utf8(request.clone()).expect("V1 request is UTF-8 JSON");
        write_json_node(&mut input, &JsonValue::String(Rc::from(request_text)));
    }
    input.push_str("]]\n");

    let output = run_lit(input.as_bytes());
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(output.stdout.ends_with(b"\n"), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn parse_v1_batch_results(raw: &str, request_count: usize) -> Vec<JsonValue> {
    let value = json_parse(raw).expect("step-4 batch result is JSON");
    let values = array(&value);
    assert_eq!(
        string(&values[0]),
        "topaz.lit-j57-2c-diagnostic-projection-batch-result/v1"
    );
    assert_eq!(values.len(), request_count + 1);
    values[1..].to_vec()
}

fn v1_batch_results(requests: &[Vec<u8>]) -> Vec<JsonValue> {
    parse_v1_batch_results(&v1_batch_raw(requests), requests.len())
}

fn v1_partitioned_results(requests: Vec<Vec<u8>>, resource_flags: Vec<bool>) -> Vec<JsonValue> {
    assert_eq!(requests.len(), resource_flags.len());
    let mut ordinary = Vec::new();
    let mut resources = Vec::new();
    for (index, (request, resource)) in requests.into_iter().zip(resource_flags).enumerate() {
        if resource {
            resources.push((index, request));
        } else {
            ordinary.push((index, request));
        }
    }

    let result_count = ordinary.len() + resources.len();
    let ordinary_batch = ordinary
        .iter()
        .map(|(_, request)| request.clone())
        .collect::<Vec<_>>();
    let ordinary_results = v1_batch_results(&ordinary_batch);
    let mut completed = ordinary
        .into_iter()
        .map(|(index, _)| index)
        .zip(ordinary_results)
        .collect::<Vec<_>>();

    let worker_count = 8.min(resources.len());
    let mut buckets = (0..worker_count).map(|_| Vec::new()).collect::<Vec<_>>();
    for (position, (index, request)) in resources.into_iter().enumerate() {
        buckets[position % worker_count].push((index, request));
    }
    std::thread::scope(|scope| {
        let handles = buckets
            .into_iter()
            .filter(|bucket| !bucket.is_empty())
            .map(|bucket| {
                scope.spawn(move || {
                    let indices = bucket.iter().map(|(index, _)| *index).collect::<Vec<_>>();
                    let batch = bucket
                        .into_iter()
                        .map(|(_, request)| request)
                        .collect::<Vec<_>>();
                    (indices, v1_batch_raw(&batch))
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            let (indices, raw) = handle.join().expect("resource projection worker completes");
            let result_count = indices.len();
            completed.extend(
                indices
                    .into_iter()
                    .zip(parse_v1_batch_results(&raw, result_count)),
            );
        }
    });
    completed.sort_by_key(|(index, _)| *index);
    assert_eq!(completed.len(), result_count);
    completed.into_iter().map(|(_, result)| result).collect()
}

fn field<'a>(value: &'a JsonValue, name: &str) -> &'a JsonValue {
    let JsonValue::Object(fields) = value else {
        panic!("expected object, got {value:?}");
    };
    fields
        .get(name)
        .unwrap_or_else(|| panic!("missing `{name}` in {value:?}"))
}

fn element(value: &JsonValue, index: usize) -> &JsonValue {
    let JsonValue::Array(values) = value else {
        panic!("expected array, got {value:?}");
    };
    &values[index]
}

fn string(value: &JsonValue) -> &str {
    let JsonValue::String(text) = value else {
        panic!("expected string, got {value:?}");
    };
    text
}

fn integer(value: &JsonValue) -> i64 {
    let JsonValue::Number(number) = value else {
        panic!("expected integer, got {value:?}");
    };
    number.int.expect("JSON number is an exact integer")
}

fn boolean(value: &JsonValue) -> bool {
    let JsonValue::Bool(flag) = value else {
        panic!("expected bool, got {value:?}");
    };
    *flag
}

fn array(value: &JsonValue) -> &[JsonValue] {
    let JsonValue::Array(values) = value else {
        panic!("expected array, got {value:?}");
    };
    values
}

fn field_object_values(value: &JsonValue) -> Vec<&JsonValue> {
    let JsonValue::Object(fields) = value else {
        panic!("expected object, got {value:?}");
    };
    fields.values().collect()
}

fn assert_exact_fields(value: &JsonValue, expected: &[&str]) {
    let JsonValue::Object(fields) = value else {
        panic!("expected object, got {value:?}");
    };
    let actual = fields.keys().map(|key| key.as_ref()).collect::<Vec<_>>();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    assert_eq!(actual, expected, "unexpected object shape: {value:#?}");
}

fn assert_field_order(raw: &str, fields: &[&str]) {
    let mut rest = raw;
    for field in fields {
        let needle = format!("\"{field}\":");
        let index = rest
            .find(&needle)
            .unwrap_or_else(|| panic!("missing ordered field `{field}` in {raw}"));
        rest = &rest[index + needle.len()..];
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(ALPHABET[(first >> 2) as usize] as char);
        output.push(ALPHABET[(((first & 3) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(ALPHABET[(((second & 15) << 2) | (third >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[(third & 63) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn sha256_utf8(bytes: &[u8]) -> String {
    topaz_package::manifest_sha256(std::str::from_utf8(bytes).expect("test channel is UTF-8"))
        .strip_prefix("sha256:")
        .unwrap()
        .to_string()
}

fn assert_byte_channel(channel: &JsonValue, expected: &[u8]) {
    assert_exact_fields(channel, &["encoding", "base64", "byte_len", "sha256"]);
    assert_eq!(string(field(channel, "encoding")), "base64");
    assert_eq!(string(field(channel, "base64")), base64(expected));
    assert_eq!(integer(field(channel, "byte_len")), expected.len() as i64);
    assert_eq!(string(field(channel, "sha256")), sha256_utf8(expected));
}

fn request_for_source(invocation: &str, logical_source: &str, source: &str) -> Vec<u8> {
    request_for_source_and_profile(
        invocation,
        logical_source,
        source,
        "lispex-receipt-144/v1",
        "[2000000,100000,2000000,5000000,1048576]",
    )
}

fn request_for_source_and_profile(
    invocation: &str,
    logical_source: &str,
    source: &str,
    profile: &str,
    limits: &str,
) -> Vec<u8> {
    let valid = String::from_utf8(fixture("backend-run-request.valid.v1.jsonl")).unwrap();
    valid
        .replacen("j57-2c-fixture-valid", invocation, 1)
        .replacen("fixture/arbitrary-source.lspx", logical_source, 1)
        .replacen("KCsgMjAgMjIpCg==", &base64(source.as_bytes()), 1)
        .replacen(
            "773c5f0cfea28543c7c36641fcf59610ff670b8c6a9435437d8fc47bf492b865",
            &sha256_utf8(source.as_bytes()),
            1,
        )
        .replacen("lispex-receipt-144/v1", profile, 1)
        .replacen("[2000000,100000,2000000,5000000,1048576]", limits, 1)
        .into_bytes()
}

fn request_for_interpreter_artifact() -> Vec<u8> {
    let bytes = std::fs::read(source()).unwrap();
    let identity = sha256_utf8(&bytes);
    let valid = String::from_utf8(fixture("backend-run-request.valid.v1.jsonl")).unwrap();
    let predecessor = "\"path\":\"libexec/topaz/lispex/lit-runner.exe\",\"byte_len\":35,\"sha256\":\"0bfa07ec008e6e164e96030f0cb3c6dc3ffd60396e9936eed52174f2c9af95d3\"";
    let successor = format!(
        "\"path\":\"evidence/topaz-interpreter/lit.tpz\",\"byte_len\":{},\"sha256\":\"{identity}\"",
        bytes.len()
    );
    let request = valid.replacen(predecessor, &successor, 1);
    assert_ne!(request, valid, "fixture artifact expectation is replaced");
    request.into_bytes()
}

fn interpreter_host_frame(
    request: &[u8],
    host_variant: &str,
    artifact_sha256: &str,
    deadline_unix_millis: &str,
    deadline_state: &str,
    binary_stdio: bool,
    fresh_process: bool,
    process_control: bool,
) -> Vec<u8> {
    let source_bytes = std::fs::read(source()).unwrap();
    let request_text = std::str::from_utf8(request).expect("V1 request is UTF-8 JSON");
    let mut encoded_request = String::new();
    write_json_node(
        &mut encoded_request,
        &JsonValue::String(Rc::from(request_text)),
    );
    format!(
        "{{\"schema\":\"topaz.lit-host-frame/v1\",\"request_jsonl\":{encoded_request},\"binding\":{{\"schema\":\"topaz.lit-host-binding/v1\",\"host_variant\":\"{host_variant}\",\"artifact\":{{\"identity_kind\":\"topaz-lit-product-artifact-bytes\",\"path\":\"evidence/topaz-interpreter/lit.tpz\",\"byte_len\":{},\"sha256\":\"{artifact_sha256}\"}},\"deadline_unix_millis\":{deadline_unix_millis},\"deadline_state\":\"{deadline_state}\",\"binary_stdio\":{binary_stdio},\"fresh_process\":{fresh_process},\"process_control\":{process_control}}}}}\n",
        source_bytes.len(),
    )
    .into_bytes()
}

fn assert_host_frame_rejected_before_source(frame: &[u8]) {
    let value = v1_result(frame);
    assert_eq!(string(field(&value, "status")), "infrastructure-error");
    let metrics = field(&value, "metrics");
    assert!(!boolean(field(metrics, "protocol_admitted")));
    assert!(!boolean(field(metrics, "source_execution_started")));
}

fn h5_source(case_id: &str) -> String {
    let request = json_parse(
        &std::fs::read_to_string(
            compiler_root()
                .join("lit/h5/fixtures")
                .join(format!("{case_id}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    let bytes = array(element(&request, 5))
        .iter()
        .map(|value| u8::try_from(integer(value)).unwrap())
        .collect::<Vec<_>>();
    String::from_utf8(bytes).unwrap()
}

fn member_keys(value: &JsonValue) -> Vec<&str> {
    array(value).iter().map(string).collect()
}

fn append_json(output: &mut String, value: &JsonValue) {
    write_json_node(output, value);
}

fn diagnostic_projection_line(case_key: &str, result: &JsonValue) -> String {
    assert!(!case_key.contains(['"', '\\']));
    let diagnostics = field(result, "diagnostics");
    assert_eq!(array(diagnostics).len(), 1, "{case_key}: {result:#?}");
    let diagnostic = element(diagnostics, 0);
    let span = field(diagnostic, "span");
    let mut output = format!("[\"{case_key}\"");
    for value in [
        field(diagnostic, "code"),
        field(diagnostic, "severity"),
        field(diagnostic, "phase"),
        field(diagnostic, "family"),
        field(diagnostic, "message"),
        field(diagnostic, "irritants"),
        field(span, "source"),
        field(span, "line"),
        field(span, "column"),
    ] {
        output.push(',');
        append_json(&mut output, value);
    }
    output.push_str("]\n");
    output
}

fn projection_commitment(members: &JsonValue, results: &BTreeMap<String, JsonValue>) -> String {
    let mut rows = String::new();
    for case_key in member_keys(members) {
        rows.push_str(&diagnostic_projection_line(
            case_key,
            results
                .get(case_key)
                .unwrap_or_else(|| panic!("missing step-4 result for {case_key}")),
        ));
    }
    sha256_utf8(rows.as_bytes())
}

fn internal_disposition(result: &JsonValue) -> &str {
    string(field(field(result, "metrics"), "internal_disposition"))
}

fn disposition_commitment(members: &JsonValue, results: &BTreeMap<String, JsonValue>) -> String {
    let mut rows = String::new();
    for case_key in member_keys(members) {
        let result = results
            .get(case_key)
            .unwrap_or_else(|| panic!("missing step-4 result for {case_key}"));
        rows.push_str(case_key);
        rows.push('\t');
        rows.push_str(internal_disposition(result));
        rows.push('\n');
    }
    sha256_utf8(rows.as_bytes())
}

fn assert_catalog_diagnostic(diagnostic: &JsonValue, logical_source: &str) {
    assert_exact_fields(
        diagnostic,
        &[
            "code",
            "severity",
            "phase",
            "family",
            "message",
            "irritants",
            "span",
            "rendered_bytes",
        ],
    );
    let catalog = json_parse(
        &std::fs::read_to_string(
            compiler_root().join("lit/contracts/lispex-1.5/diagnostic-catalog.v1.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let code = string(field(diagnostic, "code"));
    let row = array(field(&catalog, "rows"))
        .iter()
        .find(|row| string(element(row, 0)) == code)
        .unwrap_or_else(|| panic!("{code} is not a frozen diagnostic-catalog row"));
    assert_eq!(field(diagnostic, "severity"), element(row, 1));
    let catalog_phase = string(element(row, 2));
    let actual_phase = string(field(diagnostic, "phase"));
    if catalog_phase == "reader-or-normalizer" {
        assert!(matches!(actual_phase, "reader" | "normalizer"));
    } else if catalog_phase == "reader-or-runtime" {
        assert!(matches!(actual_phase, "reader" | "runtime"));
    } else {
        assert_eq!(actual_phase, catalog_phase);
    }
    assert_eq!(field(diagnostic, "family"), element(row, 3));

    let span = field(diagnostic, "span");
    assert_exact_fields(span, &["source", "line", "column"]);
    assert_eq!(string(field(span, "source")), logical_source);
    let line = integer(field(span, "line"));
    let column = integer(field(span, "column"));
    assert!(line >= 1 && column >= 1, "{diagnostic:#?}");
    let irritants = array(field(diagnostic, "irritants"));
    let mut rendered = format!(
        "{code} {logical_source}:{line}:{column} {}",
        string(field(diagnostic, "message"))
    );
    for irritant in irritants {
        rendered.push(' ');
        rendered.push_str(string(irritant));
    }
    rendered.push('\n');
    assert_byte_channel(field(diagnostic, "rendered_bytes"), rendered.as_bytes());
}

fn assert_extension_diagnostic(
    diagnostic: &JsonValue,
    logical_source: &str,
    code: &str,
    phase: &str,
    family: &str,
) {
    assert_eq!(string(field(diagnostic, "code")), code);
    assert_eq!(string(field(diagnostic, "severity")), "error");
    assert_eq!(string(field(diagnostic, "phase")), phase);
    assert_eq!(string(field(diagnostic, "family")), family);
    let span = field(diagnostic, "span");
    assert_eq!(string(field(span, "source")), logical_source);
    let mut rendered = format!(
        "{code} {logical_source}:{}:{} {}",
        integer(field(span, "line")),
        integer(field(span, "column")),
        string(field(diagnostic, "message"))
    );
    for irritant in array(field(diagnostic, "irritants")) {
        rendered.push(' ');
        rendered.push_str(string(irritant));
    }
    rendered.push('\n');
    assert_byte_channel(field(diagnostic, "rendered_bytes"), rendered.as_bytes());
}

fn assert_fail_closed(value: &JsonValue, message_fragment: &str) {
    assert_eq!(
        string(field(value, "schema")),
        "lispex.observation-result/v1"
    );
    let backend = field(value, "backend");
    assert_eq!(string(field(backend, "id")), "lit");
    assert_eq!(string(field(backend, "version")), "5.7.0");
    assert_eq!(string(field(value, "status")), "infrastructure-error");
    assert_eq!(integer(field(value, "exit_status")), 2);
    let diagnostic = element(field(value, "diagnostics"), 0);
    assert_eq!(string(field(diagnostic, "code")), "backend-protocol");
    assert!(
        string(field(diagnostic, "message")).contains(message_fragment),
        "{value:#?}"
    );
    let metrics = field(value, "metrics");
    assert!(!boolean(field(metrics, "protocol_admitted")));
    assert!(!boolean(field(metrics, "source_execution_started")));
    let fallbacks = field(field(value, "identity"), "forbidden_fallback_counts");
    let JsonValue::Object(fallbacks) = fallbacks else {
        panic!("fallback counts are not an object: {fallbacks:?}");
    };
    for count in fallbacks.values() {
        assert_eq!(integer(count), 0);
    }
}

#[test]
#[ignore = "historical projections are replay evidence, not current-product checks"]
fn targeted_historical_diagnostic_pairs_use_the_shared_projection() {
    let authority = authority();
    assert_eq!(
        string(field(&authority, "schema")),
        "topaz.j57-2c-diagnostic-pair-authority/v1"
    );
    let denominator = field(&authority, "historical_denominator");
    assert_eq!(integer(field(denominator, "case_count")), 144);
    assert_eq!(integer(field(denominator, "diagnostic_pair_count")), 182);
    assert_eq!(integer(field(denominator, "same_disposition_count")), 166);
    assert_eq!(integer(field(denominator, "status_defect_count")), 16);

    let case_sources = array(field(&authority, "case_sources"));
    assert_eq!(
        case_sources.len() as i64,
        integer(field(&authority, "case_source_count"))
    );
    assert_eq!(case_sources.len(), 104);

    let mut requests = Vec::with_capacity(case_sources.len());
    let mut resource_flags = Vec::with_capacity(case_sources.len());
    let mut case_keys = Vec::with_capacity(case_sources.len());
    let mut logical_sources = BTreeMap::new();
    for row in case_sources {
        let case_key = string(field(row, "case_key"));
        let case_index = integer(field(row, "case_index"));
        let case_id = string(field(row, "case_id"));
        let logical_source = string(field(row, "logical_source_id"));
        let profile = string(field(row, "resource_profile"));
        let source = h5_source(case_id);
        assert_eq!(
            sha256_utf8(source.as_bytes()),
            string(field(row, "source_sha256")),
            "{case_key}"
        );
        let limits = match profile {
            "lispex-receipt-144/v1" => "[2000000,100000,2000000,5000000,1048576]",
            "lispex-generated-11088/v1" => "[400000,16384,400000,400000,65536]",
            other => panic!("unexpected step-4 profile {other}"),
        };
        requests.push(request_for_source_and_profile(
            &format!("j57-2c-step-4-{case_index}"),
            logical_source,
            &source,
            profile,
            limits,
        ));
        resource_flags.push(profile == "lispex-generated-11088/v1");
        case_keys.push(case_key.to_string());
        logical_sources.insert(case_key.to_string(), logical_source.to_string());
    }

    let projected = v1_partitioned_results(requests, resource_flags);
    let mut results = BTreeMap::new();
    for (case_key, result) in case_keys.into_iter().zip(projected) {
        assert_eq!(
            string(field(&result, "schema")),
            "lispex.observation-result/v1",
            "{case_key}"
        );
        assert!(
            boolean(field(field(&result, "metrics"), "source_execution_started")),
            "{case_key}: {result:#?}"
        );
        assert_ne!(
            string(field(&result, "status")),
            "infrastructure-error",
            "{case_key}: {result:#?}"
        );
        let diagnostic = element(field(&result, "diagnostics"), 0);
        let logical_source = logical_sources.get(&case_key).unwrap();
        let code = string(field(diagnostic, "code"));
        assert_ne!(
            code, "backend-protocol",
            "unseeded diagnostic projection for {case_key}: {result:#?}"
        );
        if code == "backend-unsupported" {
            assert_extension_diagnostic(
                diagnostic,
                logical_source,
                "backend-unsupported",
                "runtime",
                "backend-unsupported",
            );
        } else if code.starts_with("HK") {
            assert_extension_diagnostic(
                diagnostic,
                logical_source,
                code,
                "runtime",
                "backend-protocol",
            );
        } else {
            assert_catalog_diagnostic(diagnostic, logical_source);
        }
        if code == "recursion-limit" {
            let span = field(diagnostic, "span");
            assert_eq!(integer(field(span, "line")), 1, "{case_key}");
            assert_eq!(integer(field(span, "column")), 1, "{case_key}");
        }
        assert!(results.insert(case_key, result).is_none());
    }
    assert_eq!(results.len(), 104);

    let rust_lit = field(&authority, "rust_lit");
    assert_eq!(integer(field(rust_lit, "member_count")), 39);
    assert_eq!(array(field(rust_lit, "members")).len(), 39);
    assert_eq!(integer(field(rust_lit, "same_disposition_count")), 31);
    assert_eq!(array(field(rust_lit, "same_disposition_members")).len(), 31);
    for case_key in member_keys(field(rust_lit, "members")) {
        assert!(
            results.contains_key(case_key),
            "unexercised Rust/LIT {case_key}"
        );
    }
    assert_eq!(
        projection_commitment(field(rust_lit, "same_disposition_members"), &results),
        string(field(rust_lit, "canonical_projection_sha256"))
    );
    assert_eq!(
        disposition_commitment(field(rust_lit, "same_disposition_members"), &results),
        string(field(rust_lit, "disposition_sha256"))
    );
    assert_eq!(integer(field(rust_lit, "canonical_projection_count")), 31);

    let status_defects = array(field(rust_lit, "status_defects"));
    assert_eq!(integer(field(rust_lit, "status_defect_count")), 8);
    assert_eq!(status_defects.len(), 8);
    for defect in status_defects {
        let case_key = string(field(defect, "case_key"));
        assert_eq!(
            string(field(defect, "rust_disposition")),
            "runtime-error",
            "{case_key}"
        );
        assert_eq!(
            internal_disposition(results.get(case_key).unwrap()),
            string(field(defect, "lit_disposition")),
            "status defect was rewritten for {case_key}"
        );
    }

    let lil_lit = field(&authority, "lil_lit");
    assert_eq!(integer(field(lil_lit, "member_count")), 103);
    assert_eq!(array(field(lil_lit, "members")).len(), 103);
    assert_eq!(integer(field(lil_lit, "same_disposition_count")), 103);
    for case_key in member_keys(field(lil_lit, "members")) {
        assert!(
            results.contains_key(case_key),
            "unexercised LIL/LIT {case_key}"
        );
    }
    assert_eq!(
        projection_commitment(field(lil_lit, "members"), &results),
        string(field(lil_lit, "canonical_projection_sha256"))
    );
    assert_eq!(
        disposition_commitment(field(lil_lit, "members"), &results),
        string(field(lil_lit, "disposition_sha256"))
    );
    assert_eq!(integer(field(lil_lit, "canonical_projection_count")), 103);

    let deferred = field(&authority, "deferred_resources");
    assert_eq!(integer(field(deferred, "count")), 5);
    assert_eq!(string(field(deferred, "deferred_to")), "J57-2D");
    assert!(boolean(field(deferred, "projection_exercised_in_step_4")));
    assert!(!boolean(field(deferred, "resource_behavior_closed")));
    for case_key in member_keys(field(deferred, "members")) {
        let result = results.get(case_key).unwrap();
        assert_eq!(internal_disposition(result), "resource", "{case_key}");
        assert_eq!(
            string(field(element(field(result, "diagnostics"), 0), "code")),
            "recursion-limit",
            "{case_key}"
        );
    }
}

#[test]
fn canonical_v1_request_executes_and_projects_all_observation_fields() {
    let request = fixture("backend-run-request.valid.v1.jsonl");
    let expected = json_parse(std::str::from_utf8(&request).unwrap()).unwrap();
    let (value, raw) = v1_result_with_raw(&request);

    assert_exact_fields(
        &value,
        &[
            "schema",
            "invocation_id",
            "backend",
            "status",
            "stdout_bytes",
            "diagnostics",
            "warnings",
            "exit_status",
            "value_projection",
            "resource_outcome",
            "metrics",
            "identity",
        ],
    );
    assert_field_order(
        &raw,
        &[
            "schema",
            "invocation_id",
            "backend",
            "status",
            "stdout_bytes",
            "diagnostics",
            "warnings",
            "exit_status",
            "value_projection",
            "resource_outcome",
            "metrics",
            "identity",
        ],
    );
    assert_eq!(
        string(field(&value, "schema")),
        "lispex.observation-result/v1"
    );
    assert_eq!(
        field(&value, "invocation_id"),
        field(&expected, "invocation_id")
    );
    let backend = field(&value, "backend");
    assert_exact_fields(backend, &["id", "version", "profile", "host_variant"]);
    assert_eq!(string(field(backend, "id")), "lit");
    assert_eq!(string(field(backend, "version")), "5.7.0");
    assert_eq!(string(field(backend, "profile")), "lispex-profile-1.5");
    assert_eq!(
        string(field(backend, "host_variant")),
        "topaz-interpreter-protocol-v1"
    );
    assert_eq!(string(field(&value, "status")), "ok");
    assert_byte_channel(field(&value, "stdout_bytes"), b"42\n");
    assert!(array(field(&value, "diagnostics")).is_empty());
    assert!(array(field(&value, "warnings")).is_empty());
    assert_eq!(integer(field(&value, "exit_status")), 0);
    let values = field(&value, "value_projection");
    assert_exact_fields(values, &["schema", "values"]);
    assert_eq!(
        string(field(values, "schema")),
        "lispex.value-projection/v1"
    );
    let projected = array(field(values, "values"));
    assert_eq!(projected.len(), 1);
    assert_eq!(string(&projected[0]), "42");

    let metrics = field(&value, "metrics");
    assert!(boolean(field(metrics, "protocol_admitted")));
    assert!(boolean(field(metrics, "source_execution_started")));
    assert!(boolean(field(metrics, "kernel_produced")));
    assert!(boolean(field(metrics, "transition_metrics_available")));
    assert!(!boolean(field(
        metrics,
        "artifact_identity_verified_by_executing_host"
    )));
    assert_eq!(integer(field(metrics, "capability_row_count")), 205);
    assert_eq!(integer(field(metrics, "rust_supported_count")), 205);
    assert_eq!(integer(field(metrics, "lil_supported_count")), 84);
    assert_eq!(integer(field(metrics, "lit_supported_count")), 205);
    assert_eq!(
        string(field(metrics, "value_projection_source")),
        "common-machine-completed-root-auto-print-bridge/v1"
    );

    let identity = field(&value, "identity");
    assert_exact_fields(
        identity,
        &[
            "source_sha256",
            "artifact",
            "profile_id",
            "meter_id",
            "capability_manifest_id",
            "capability_manifest_digest",
            "meter_manifest_digest",
            "observation_schema_id",
            "observation_contract_id",
            "observation_contract_digest",
            "capability_schema",
            "observation_schema",
            "resource_schema",
            "forbidden_fallback_counts",
        ],
    );
    assert_eq!(
        field(identity, "source_sha256"),
        field(&expected, "source_sha256")
    );
    let expectation = field(&expected, "artifact_expectation");
    assert_eq!(field(identity, "artifact"), field(expectation, "artifact"));
    assert_eq!(string(field(identity, "profile_id")), "lispex-profile-1.5");
    assert_eq!(
        string(field(identity, "meter_id")),
        "lispex.product-resource-meter/v1"
    );
    assert_eq!(
        string(field(identity, "capability_manifest_id")),
        "lispex.capability-manifest/2"
    );
    assert_eq!(
        string(field(identity, "capability_manifest_digest")),
        "sha256:22debb0569cabc51c8848d5a089e1b3c0b88e6c606525b92b5ce060e40f1d900"
    );
    assert_eq!(
        string(field(identity, "meter_manifest_digest")),
        "sha256:0e7bfe9be2ea155d7f9d8b5f83f49b0df4451710891769b77423e1c7cecbf657"
    );
    assert_eq!(
        string(field(identity, "observation_schema_id")),
        "lispex.observation-result/v1"
    );
    assert_eq!(
        string(field(identity, "observation_contract_id")),
        "lispex-observation-contract/1"
    );
    assert_eq!(
        string(field(identity, "observation_contract_digest")),
        "sha256:efb521008ae1077163b55bcb0f77e03fa586bd047ead2ee9b7f5fa25e4a6c7f8"
    );
    assert_eq!(
        string(field(identity, "capability_schema")),
        "lispex.primitive-capabilities/v2"
    );
    assert_eq!(
        string(field(identity, "observation_schema")),
        "lispex.observation-result/v1"
    );
    assert_eq!(
        string(field(identity, "resource_schema")),
        "lispex.resource-profiles/v1"
    );
    let fallbacks = field(identity, "forbidden_fallback_counts");
    assert_exact_fields(
        fallbacks,
        &[
            "debug_binary",
            "host_apply",
            "host_callback",
            "host_control",
            "host_eval",
            "host_source_decoder",
            "runtime_download",
            "rust_backend",
            "sibling_checkout",
        ],
    );
    for count in field_object_values(fallbacks) {
        assert_eq!(integer(count), 0);
    }

    let resource = field(&value, "resource_outcome");
    assert_exact_fields(
        resource,
        &["profile_id", "purpose", "status", "limit", "limits"],
    );
    assert_eq!(
        string(field(resource, "profile_id")),
        "lispex-receipt-144/v1"
    );
    assert_eq!(string(field(resource, "purpose")), "bounded-evidence");
    assert_eq!(string(field(resource, "status")), "within-limit");
    assert_eq!(field(resource, "limit"), &JsonValue::Null);
    let limits = field(resource, "limits");
    assert_exact_fields(
        limits,
        &[
            "frontend_transition_limit",
            "token_limit",
            "normalization_step_limit",
            "machine_transition_limit",
            "output_byte_limit",
        ],
    );
    assert_eq!(
        integer(field(limits, "frontend_transition_limit")),
        2_000_000
    );
    assert_eq!(integer(field(limits, "token_limit")), 100_000);
    assert_eq!(
        integer(field(limits, "normalization_step_limit")),
        2_000_000
    );
    assert_eq!(
        integer(field(limits, "machine_transition_limit")),
        5_000_000
    );
    assert_eq!(integer(field(limits, "output_byte_limit")), 1_048_576);
}

#[test]
fn guest_primitive_fault_is_runtime_error_and_preserves_committed_stdout() {
    let source = h5_source(
        "041-conformance_doc-evaluation-program_doc.program.reference.functional.library",
    );
    let value = v1_result(&request_for_source(
        "lit-guest-fault-041",
        "L120-SDL-041.lspx",
        &source,
    ));

    assert_eq!(string(field(&value, "status")), "runtime-error");
    assert_eq!(integer(field(&value, "exit_status")), 1);
    assert_byte_channel(
        field(&value, "stdout_bytes"),
        b"(1 4 9 16)\n(8 10 7)\n100\n",
    );
    let diagnostics = array(field(&value, "diagnostics"));
    assert_eq!(diagnostics.len(), 1);
    assert_extension_diagnostic(
        &diagnostics[0],
        "L120-SDL-041.lspx",
        "HK257",
        "runtime",
        "backend-protocol",
    );
    let span = field(&diagnostics[0], "span");
    assert_eq!(integer(field(span, "line")), 73);
    assert_eq!(integer(field(span, "column")), 18);
}

#[test]
fn ratified_reader_normalizer_and_demanded_lookup_cases_match_public_projections() {
    let cases = [
        (
            "043-conformance_doc-evaluation-program_doc.program.concepts.control.flow",
            "ok",
            "",
            "",
            0,
        ),
        (
            "052-conformance_diagnostic-negative_diag.negative.002",
            "source-error",
            "E120",
            "normalizer",
            1,
        ),
        (
            "053-conformance_diagnostic-negative_diag.negative.003",
            "source-error",
            "E120",
            "reader",
            1,
        ),
        (
            "054-conformance_diagnostic-negative_diag.negative.004",
            "source-error",
            "E130",
            "normalizer",
            1,
        ),
        (
            "076-conformance_normalization-pair-source_norm.pair.004",
            "runtime-error",
            "E300",
            "runtime",
            1,
        ),
    ];
    let requests = cases
        .iter()
        .enumerate()
        .map(|(index, (case, _, _, _, _))| {
            let source = h5_source(case);
            request_for_source(
                &format!("lit-ratified-{index}"),
                &format!("{case}.lspx"),
                &source,
            )
        })
        .collect::<Vec<_>>();
    let results = v1_batch_results(&requests);

    for ((case, status, code, phase, exit_status), result) in cases.iter().zip(&results) {
        assert_eq!(string(field(&result, "status")), *status, "{case}");
        assert_eq!(
            integer(field(&result, "exit_status")),
            *exit_status,
            "{case}"
        );
        let diagnostics = array(field(&result, "diagnostics"));
        if code.is_empty() {
            assert!(diagnostics.is_empty(), "{case}: {result:#?}");
        } else {
            assert_eq!(diagnostics.len(), 1, "{case}: {result:#?}");
            assert_eq!(string(field(&diagnostics[0], "code")), *code, "{case}");
            assert_eq!(string(field(&diagnostics[0], "phase")), *phase, "{case}");
        }
    }
    assert_byte_channel(
        field(&results[0], "stdout_bytes"),
        b"\"Access Granted\"\n#f\n100\n",
    );
    let lookup_span = field(&array(field(&results[4], "diagnostics"))[0], "span");
    assert_eq!(integer(field(lookup_span, "line")), 1);
    assert_eq!(integer(field(lookup_span, "column")), 6);
}

#[test]
fn malformed_v1_handshakes_fail_before_source_execution() {
    let valid = String::from_utf8(fixture("backend-run-request.valid.v1.jsonl")).unwrap();
    let body = valid.strip_suffix('\n').unwrap();
    let reordered = body.replacen(
        "\"schema\":\"lispex.backend-run-request/v1\",\"invocation_id\":\"j57-2c-fixture-valid\",\"profile\":\"lispex-profile-1.5\"",
        "\"schema\":\"lispex.backend-run-request/v1\",\"profile\":\"lispex-profile-1.5\",\"invocation_id\":\"j57-2c-fixture-valid\"",
        1,
    );
    let cases = [
        (
            "reordered-fields",
            format!("{reordered}\n"),
            "missing, extra, or reordered",
        ),
        (
            "profile-skew",
            valid.replacen(
                "\"profile\":\"lispex-profile-1.5\"",
                "\"profile\":\"lispex-profile-1.4\"",
                1,
            ),
            "binding is incompatible",
        ),
        (
            "invalid-base64",
            valid.replacen("KCsgMjAgMjIpCg==", "%%%", 1),
            "source base64 is invalid",
        ),
        (
            "source-digest",
            valid.replacen(
                "773c5f0cfea28543c7c36641fcf59610ff670b8c6a9435437d8fc47bf492b865",
                &"0".repeat(64),
                1,
            ),
            "source bytes do not match",
        ),
        (
            "resource-limits",
            valid.replacen(
                "[2000000,100000,2000000,5000000,1048576]",
                "[2000001,100000,2000000,5000000,1048576]",
                1,
            ),
            "limits do not match",
        ),
        (
            "artifact-kind",
            valid.replacen(
                "topaz-lit-product-artifact-bytes",
                "executed-native-binary-bytes",
                1,
            ),
            "artifact expectation is invalid",
        ),
        (
            "artifact-path",
            valid.replacen(
                "libexec/topaz/lispex/lit-runner.exe",
                "../lit-runner.exe",
                1,
            ),
            "artifact expectation is invalid",
        ),
        (
            "logical-source-empty-segment",
            valid.replacen(
                "fixture/arbitrary-source.lspx",
                "fixture//arbitrary-source.lspx",
                1,
            ),
            "logical source id is invalid",
        ),
        (
            "pre-cancelled",
            valid.replacen("\"abort_requested\":false", "\"abort_requested\":true", 1),
            "cancelled before source execution",
        ),
        (
            "deadline-needs-host",
            valid.replacen(
                "\"deadline_unix_millis\":null",
                "\"deadline_unix_millis\":1",
                1,
            ),
            "deadline requires host verification",
        ),
    ];

    for (label, request, expected) in cases {
        let value = v1_result(request.as_bytes());
        assert_fail_closed(&value, expected);
        assert_eq!(
            string(field(&value, "invocation_id")),
            "invalid",
            "{label}: {value:#?}"
        );
        assert_eq!(
            field(field(&value, "identity"), "artifact"),
            &JsonValue::Null,
            "{label}: {value:#?}"
        );
    }
}

#[test]
fn malformed_v1_jsonl_framing_is_structured_and_fail_closed() {
    let valid = fixture("backend-run-request.valid.v1.jsonl");
    let mut missing_lf = valid.clone();
    missing_lf.pop();
    let mut extra_lf = valid.clone();
    extra_lf.push(b'\n');
    let mut multiple = valid.clone();
    multiple.extend_from_slice(&valid);

    let mut leading_lf = b"\n".to_vec();
    leading_lf.extend_from_slice(&valid);
    let mut leading_cr = b"\r".to_vec();
    leading_cr.extend_from_slice(&valid);

    for request in [
        missing_lf,
        extra_lf,
        multiple,
        leading_lf,
        leading_cr,
        b"{]\n".to_vec(),
    ] {
        let value = v1_result(&request);
        assert_fail_closed(&value, "protocol");
    }
}

#[test]
fn frontend_and_machine_diagnostics_keep_shifted_source_spans_and_irritants() {
    let frontend_source = "\n\n  (lambda (if) if)\n";
    let frontend = v1_result(&request_for_source(
        "shifted-frontend",
        "shifted/frontend.lspx",
        frontend_source,
    ));
    assert_eq!(string(field(&frontend, "status")), "source-error");
    assert_eq!(integer(field(&frontend, "exit_status")), 1);
    assert_byte_channel(field(&frontend, "stdout_bytes"), b"");
    let frontend_diagnostic = element(field(&frontend, "diagnostics"), 0);
    assert_catalog_diagnostic(frontend_diagnostic, "shifted/frontend.lspx");
    assert_eq!(string(field(frontend_diagnostic, "code")), "E110");
    let frontend_span = field(frontend_diagnostic, "span");
    assert_eq!(integer(field(frontend_span, "line")), 3);
    assert_eq!(integer(field(frontend_span, "column")), 12);

    let runtime_source = "\n  (car 1)\n";
    let runtime = v1_result(&request_for_source(
        "shifted-runtime",
        "shifted/runtime.lspx",
        runtime_source,
    ));
    assert_eq!(string(field(&runtime, "status")), "runtime-error");
    assert_eq!(integer(field(&runtime, "exit_status")), 1);
    let runtime_diagnostic = element(field(&runtime, "diagnostics"), 0);
    assert_catalog_diagnostic(runtime_diagnostic, "shifted/runtime.lspx");
    assert_eq!(string(field(runtime_diagnostic, "code")), "E310");
    let runtime_span = field(runtime_diagnostic, "span");
    assert_eq!(integer(field(runtime_span, "line")), 2);
    assert_eq!(integer(field(runtime_span, "column")), 3);

    let error_source = "\n  (error \"boom\" 1 \"x\")\n";
    let user_error = v1_result(&request_for_source(
        "shifted-user-error",
        "shifted/user-error.lspx",
        error_source,
    ));
    let user_diagnostic = element(field(&user_error, "diagnostics"), 0);
    assert_catalog_diagnostic(user_diagnostic, "shifted/user-error.lspx");
    assert_eq!(string(field(user_diagnostic, "code")), "E330");
    assert_eq!(string(field(user_diagnostic, "message")), "boom");
    let irritants = array(field(user_diagnostic, "irritants"));
    assert_eq!(irritants.len(), 2);
    assert_eq!(string(&irritants[0]), "1");
    assert_eq!(string(&irritants[1]), "\"x\"");
    let user_span = field(user_diagnostic, "span");
    assert_eq!(integer(field(user_span, "line")), 2);
    assert_eq!(integer(field(user_span, "column")), 3);
}

#[test]
fn diagnostic_projection_is_label_independent_and_span_sensitive() {
    let source = "(car 1)\n";
    let shifted_source = "\n   (car 1)\n";
    let requests = [
        request_for_source("projection-baseline", "baseline/plain.lspx", source),
        request_for_source(
            "projection-arbitrary-label",
            "arbitrary/deep-name_47.lspx",
            source,
        ),
        request_for_source(
            "projection-shifted-span",
            "arbitrary/shifted-name_93.lspx",
            shifted_source,
        ),
    ];
    let results = v1_batch_results(&requests);
    let baseline = element(field(&results[0], "diagnostics"), 0);
    let relabeled = element(field(&results[1], "diagnostics"), 0);
    let shifted = element(field(&results[2], "diagnostics"), 0);

    for diagnostic in [baseline, relabeled, shifted] {
        assert_eq!(string(field(diagnostic, "code")), "E310");
        assert_eq!(string(field(diagnostic, "phase")), "runtime");
        assert_eq!(string(field(diagnostic, "family")), "pair-expected");
        assert_eq!(
            string(field(diagnostic, "message")),
            "car: expected a pair, got 1"
        );
        assert!(array(field(diagnostic, "irritants")).is_empty());
    }
    for field_name in [
        "code",
        "severity",
        "phase",
        "family",
        "message",
        "irritants",
    ] {
        assert_eq!(field(baseline, field_name), field(relabeled, field_name));
        assert_eq!(field(baseline, field_name), field(shifted, field_name));
    }

    let baseline_span = field(baseline, "span");
    let relabeled_span = field(relabeled, "span");
    let shifted_span = field(shifted, "span");
    assert_eq!(
        string(field(baseline_span, "source")),
        "baseline/plain.lspx"
    );
    assert_eq!(integer(field(baseline_span, "line")), 1);
    assert_eq!(integer(field(baseline_span, "column")), 1);
    assert_eq!(
        string(field(relabeled_span, "source")),
        "arbitrary/deep-name_47.lspx"
    );
    assert_eq!(integer(field(relabeled_span, "line")), 1);
    assert_eq!(integer(field(relabeled_span, "column")), 1);
    assert_eq!(
        string(field(shifted_span, "source")),
        "arbitrary/shifted-name_93.lspx"
    );
    assert_eq!(integer(field(shifted_span, "line")), 2);
    assert_eq!(integer(field(shifted_span, "column")), 4);
    assert_ne!(
        field(baseline, "rendered_bytes"),
        field(relabeled, "rendered_bytes")
    );
    assert_ne!(
        field(baseline, "rendered_bytes"),
        field(shifted, "rendered_bytes")
    );
}

#[test]
fn single_value_context_faults_keep_the_consumed_expression_origin() {
    let cases = [
        (
            "operator",
            "\n  (((lambda ()\n      (values + -))) 1 2)\n",
            2,
            4,
        ),
        (
            "operand",
            "\n  (+ ((lambda ()\n       (values 1 2))) 3)\n",
            2,
            6,
        ),
        (
            "if-test",
            "\n  (if ((lambda ()\n        (values #t #f)))\n      1\n      0)\n",
            2,
            7,
        ),
        (
            "set-rhs",
            "\n  (define x 0)\n  (set! x\n    ((lambda ()\n       (values 1 2))))\n",
            4,
            5,
        ),
        (
            "letrec-initializer",
            "\n  (letrec (\n    (x\n      ((lambda ()\n         (values 1 2)))))\n    x)\n",
            4,
            7,
        ),
        (
            "guard-test",
            "\n  (guard (e\n    (((lambda () (values #t #f))) 1))\n    (raise 'boom))\n",
            3,
            6,
        ),
    ];

    for (label, source, line, column) in cases {
        let logical_source = format!("origins/{label}.lspx");
        let value = v1_result(&request_for_source(label, &logical_source, source));
        assert_eq!(string(field(&value, "status")), "runtime-error", "{label}");
        let diagnostic = element(field(&value, "diagnostics"), 0);
        assert_catalog_diagnostic(diagnostic, &logical_source);
        assert_eq!(string(field(diagnostic, "code")), "E320", "{label}");
        let span = field(diagnostic, "span");
        assert_eq!(integer(field(span, "line")), line, "{label}");
        assert_eq!(integer(field(span, "column")), column, "{label}");
    }
}

#[test]
fn callback_and_cleanup_faults_keep_enclosing_or_signal_origins() {
    let callback_cases = [
        (
            "call-with-values",
            "\n  (call-with-values\n    (lambda ()\n      (values 10 3))\n    %)\n",
            2,
            3,
        ),
        (
            "dynamic-wind-thunk",
            "\n  (dynamic-wind\n    (lambda () 0)\n    %\n    (lambda () 1))\n",
            2,
            3,
        ),
        (
            "dynamic-wind-after",
            "\n  (dynamic-wind\n    (lambda () 0)\n    (lambda () 1)\n    %)\n",
            2,
            3,
        ),
    ];
    for (label, source, line, column) in callback_cases {
        let logical_source = format!("callbacks/{label}.lspx");
        let value = v1_result(&request_for_source(label, &logical_source, source));
        assert_eq!(string(field(&value, "status")), "runtime-error", "{label}");
        let diagnostic = element(field(&value, "diagnostics"), 0);
        assert_extension_diagnostic(
            diagnostic,
            &logical_source,
            "backend-unsupported",
            "runtime",
            "backend-unsupported",
        );
        let warning = element(field(&value, "warnings"), 0);
        assert_catalog_diagnostic(warning, &logical_source);
        for record in [diagnostic, warning] {
            let span = field(record, "span");
            assert_eq!(integer(field(span, "line")), line, "{label}");
            assert_eq!(integer(field(span, "column")), column, "{label}");
        }
    }

    let unwind_source = "\n  (guard (condition (else 0))\n    (dynamic-wind\n      (lambda () 0)\n      (lambda () (error \"boom\"))\n      %))\n";
    let unwind = v1_result(&request_for_source(
        "dynamic-wind-unwind",
        "callbacks/dynamic-wind-unwind.lspx",
        unwind_source,
    ));
    assert_eq!(string(field(&unwind, "status")), "ok");
    let warning = element(field(&unwind, "warnings"), 0);
    assert_catalog_diagnostic(warning, "callbacks/dynamic-wind-unwind.lspx");
    let warning_span = field(warning, "span");
    assert_eq!(integer(field(warning_span, "line")), 3);
    assert_eq!(integer(field(warning_span, "column")), 5);

    let handler_source = "\n  (with-exception-handler\n    (lambda (condition) 0)\n    (lambda ()\n      (raise 'boom)))\n";
    let handler = v1_result(&request_for_source(
        "noncontinuable-handler",
        "callbacks/noncontinuable-handler.lspx",
        handler_source,
    ));
    assert_eq!(string(field(&handler, "status")), "runtime-error");
    let diagnostic = element(field(&handler, "diagnostics"), 0);
    assert_catalog_diagnostic(diagnostic, "callbacks/noncontinuable-handler.lspx");
    assert_eq!(string(field(diagnostic, "code")), "E332");
    let diagnostic_span = field(diagnostic, "span");
    assert_eq!(integer(field(diagnostic_span, "line")), 5);
    assert_eq!(integer(field(diagnostic_span, "column")), 7);
}

#[test]
fn warnings_are_machine_native_ordered_and_unsupported_stays_runtime_error() {
    let source = "(guard (e (else (list-first '(1 2)))) (% 10 3))\n";
    let value = v1_result(&request_for_source(
        "warning-order",
        "warnings/order.lspx",
        source,
    ));
    assert_eq!(string(field(&value, "status")), "runtime-error");
    assert_eq!(integer(field(&value, "exit_status")), 1);
    assert_eq!(
        string(field(field(&value, "metrics"), "internal_disposition")),
        "unsupported-loud"
    );
    let diagnostic = element(field(&value, "diagnostics"), 0);
    assert_extension_diagnostic(
        diagnostic,
        "warnings/order.lspx",
        "backend-unsupported",
        "runtime",
        "backend-unsupported",
    );
    let warnings = array(field(&value, "warnings"));
    assert_eq!(warnings.len(), 2, "{value:#?}");
    assert_catalog_diagnostic(&warnings[0], "warnings/order.lspx");
    assert_catalog_diagnostic(&warnings[1], "warnings/order.lspx");
    assert_eq!(string(field(&warnings[0], "code")), "W330");
    assert_eq!(string(field(&warnings[1], "code")), "W331");
    assert_eq!(
        string(field(&warnings[0], "message")),
        "`%` is a deprecated alias of `modulo` — prefer `modulo`"
    );
    assert_eq!(
        string(field(&warnings[1], "message")),
        "`list-first` is a deprecated alias of `first` (car) — prefer `first`"
    );

    let repeated_source = "(let loop ((n 2))\n  (if (= n 0)\n      0\n      (guard (e (else (loop (- n 1))))\n        (% 10 3))))\n";
    let repeated = v1_result(&request_for_source(
        "warning-dedup",
        "warnings/dedup.lspx",
        repeated_source,
    ));
    assert_eq!(string(field(&repeated, "status")), "ok");
    assert_byte_channel(field(&repeated, "stdout_bytes"), b"0\n");
    let repeated_warnings = array(field(&repeated, "warnings"));
    assert_eq!(repeated_warnings.len(), 1, "{repeated:#?}");
    assert_eq!(string(field(&repeated_warnings[0], "code")), "W330");
    assert_catalog_diagnostic(&repeated_warnings[0], "warnings/dedup.lspx");
}

#[test]
fn completed_root_values_share_the_disclosed_auto_print_bridge() {
    let source = "(+ 1 2)\n(values 4 5)\n";
    let value = v1_result(&request_for_source(
        "multi-root-values",
        "values/multi-root.lspx",
        source,
    ));
    assert_eq!(string(field(&value, "status")), "ok");
    assert_byte_channel(field(&value, "stdout_bytes"), b"3\n4\n5\n");
    let values = array(field(field(&value, "value_projection"), "values"));
    assert_eq!(values.len(), 3);
    assert_eq!(string(&values[0]), "3");
    assert_eq!(string(&values[1]), "4");
    assert_eq!(string(&values[2]), "5");
    assert_eq!(
        integer(field(field(&value, "metrics"), "roots_completed")),
        2
    );
}

#[test]
fn named_output_resource_clears_stdout_without_erasing_completed_values() {
    let source = format!("\"{}\"\n", "a".repeat(70_000));
    let value = v1_result(&request_for_source_and_profile(
        "named-output-resource",
        "resource/output.lspx",
        &source,
        "lispex-generated-11088/v1",
        "[400000,16384,400000,400000,65536]",
    ));
    assert_eq!(string(field(&value, "status")), "resource");
    assert_eq!(integer(field(&value, "exit_status")), 2);
    assert_byte_channel(field(&value, "stdout_bytes"), b"");
    let diagnostic = element(field(&value, "diagnostics"), 0);
    assert_extension_diagnostic(
        diagnostic,
        "resource/output.lspx",
        "output-limit",
        "resource",
        "output-byte-limit",
    );
    let resource = field(&value, "resource_outcome");
    assert_eq!(
        string(field(resource, "profile_id")),
        "lispex-generated-11088/v1"
    );
    assert_eq!(string(field(resource, "purpose")), "bounded-evidence");
    assert_eq!(string(field(resource, "status")), "limit-exceeded");
    assert_eq!(string(field(resource, "limit")), "output-bytes");
    let values = array(field(field(&value, "value_projection"), "values"));
    assert_eq!(values.len(), 1);
    assert_eq!(string(&values[0]).len(), 70_002);
}

#[test]
fn named_machine_transition_resource_reports_the_enforced_limit() {
    let source = "(let loop () (loop))\n";
    let value = v1_result(&request_for_source_and_profile(
        "named-machine-resource",
        "resource/machine-transitions.lspx",
        source,
        "lispex-generated-11088/v1",
        "[400000,16384,400000,400000,65536]",
    ));
    assert_eq!(string(field(&value, "status")), "resource");
    assert_eq!(integer(field(&value, "exit_status")), 2);
    assert_byte_channel(field(&value, "stdout_bytes"), b"");
    let diagnostic = element(field(&value, "diagnostics"), 0);
    assert_catalog_diagnostic(diagnostic, "resource/machine-transitions.lspx");
    assert_eq!(string(field(diagnostic, "code")), "recursion-limit");
    let resource = field(&value, "resource_outcome");
    assert_eq!(string(field(resource, "status")), "limit-exceeded");
    assert_eq!(string(field(resource, "limit")), "machine-transitions");
}

#[test]
#[ignore = "the exact adapter replay remains sealed at its historical product commit"]
fn historical_interpreter_adapter_binds_actual_source_and_replays_exactly() {
    let request = request_for_interpreter_artifact();
    let first = run_historical_interpreter_adapter(&request);
    let replay = run_historical_interpreter_adapter(&request);
    assert!(first.status.success(), "{first:?}");
    assert!(first.stderr.is_empty(), "{first:?}");
    assert_eq!(first.stdout, replay.stdout, "adapter replay bytes differ");
    assert!(first.stdout.ends_with(b"\n"), "{first:?}");
    assert!(
        !first.stdout[..first.stdout.len() - 1].contains(&b'\n'),
        "{first:?}"
    );

    let value = json_parse(std::str::from_utf8(&first.stdout).unwrap()).unwrap();
    assert_eq!(string(field(&value, "status")), "ok");
    assert_eq!(
        string(field(field(&value, "backend"), "host_variant")),
        "topaz-interpreter"
    );
    let metrics = field(&value, "metrics");
    for name in [
        "artifact_identity_verified_by_executing_host",
        "binary_stdio",
        "fresh_process",
        "deadline_process_control",
    ] {
        assert!(boolean(field(metrics, name)), "metric `{name}` is not true");
    }
    let bytes = std::fs::read(source()).unwrap();
    let artifact = field(field(&value, "identity"), "artifact");
    assert_eq!(
        string(field(artifact, "identity_kind")),
        "topaz-lit-product-artifact-bytes"
    );
    assert_eq!(
        string(field(artifact, "path")),
        "evidence/topaz-interpreter/lit.tpz"
    );
    assert_eq!(integer(field(artifact, "byte_len")), bytes.len() as i64);
    assert_eq!(string(field(artifact, "sha256")), sha256_utf8(&bytes));

    let mismatched = String::from_utf8(request)
        .unwrap()
        .replacen(&sha256_utf8(&bytes), &"0".repeat(64), 1)
        .into_bytes();
    let rejected = run_historical_interpreter_adapter(&mismatched);
    assert!(rejected.status.success(), "{rejected:?}");
    assert!(rejected.stderr.is_empty(), "{rejected:?}");
    let rejected_value = json_parse(std::str::from_utf8(&rejected.stdout).unwrap()).unwrap();
    assert_eq!(
        string(field(&rejected_value, "status")),
        "infrastructure-error"
    );
    let rejected_metrics = field(&rejected_value, "metrics");
    assert!(!boolean(field(rejected_metrics, "protocol_admitted")));
    assert!(!boolean(field(
        rejected_metrics,
        "source_execution_started"
    )));

    let request = request_for_interpreter_artifact();
    let source_digest = sha256_utf8(&std::fs::read(source()).unwrap());
    let valid_frame = interpreter_host_frame(
        &request,
        "topaz-interpreter",
        &source_digest,
        "null",
        "none",
        true,
        true,
        true,
    );
    let valid = v1_result(&valid_frame);
    assert_eq!(string(field(&valid, "status")), "ok");
    assert!(boolean(field(
        field(&valid, "metrics"),
        "artifact_identity_verified_by_executing_host"
    )));

    let invalid_frames = [
        interpreter_host_frame(
            &request,
            "rust-backend",
            &source_digest,
            "null",
            "none",
            true,
            true,
            true,
        ),
        interpreter_host_frame(
            &request,
            "topaz-interpreter",
            &"0".repeat(64),
            "null",
            "none",
            true,
            true,
            true,
        ),
        interpreter_host_frame(
            &request,
            "topaz-interpreter",
            &source_digest,
            "null",
            "none",
            false,
            true,
            true,
        ),
        interpreter_host_frame(
            &request,
            "topaz-interpreter",
            &source_digest,
            "null",
            "none",
            true,
            false,
            true,
        ),
        interpreter_host_frame(
            &request,
            "topaz-interpreter",
            &source_digest,
            "null",
            "none",
            true,
            true,
            false,
        ),
        interpreter_host_frame(
            &request,
            "topaz-interpreter",
            &source_digest,
            "null",
            "active",
            true,
            true,
            true,
        ),
        interpreter_host_frame(
            &request,
            "topaz-interpreter",
            &source_digest,
            "1",
            "none",
            true,
            true,
            true,
        ),
    ];
    for frame in invalid_frames {
        assert_host_frame_rejected_before_source(&frame);
    }
}

#[test]
fn process_control_and_four_host_validation_remain_green() {
    let adapter = compiler_root().join("lit/j57-2c/adapters/host-adapter.mjs");
    let control = Command::new("node")
        .arg(&adapter)
        .arg("--self-test-process-control")
        .current_dir(compiler_root().parent().unwrap())
        .output()
        .expect("adapter process-control self-test starts");
    assert!(control.status.success(), "{control:?}");
    assert!(control.stderr.is_empty(), "{control:?}");
    assert_eq!(
        control.stdout,
        b"J57-2C host process-control self-test passed\n"
    );

    let validation = Command::new("node")
        .arg(compiler_root().join("lit/j57-2c/check-historical-step-6.mjs"))
        .current_dir(compiler_root().parent().unwrap())
        .output()
        .expect("historical predecessor checker starts");
    assert!(validation.status.success(), "{validation:?}");
    assert!(validation.stderr.is_empty(), "{validation:?}");
    assert!(
        String::from_utf8_lossy(&validation.stdout)
            .contains("J57-2C historical step-6 validation passed"),
        "{validation:?}"
    );
}

#[test]
fn immutable_instance_and_replay_validations_remain_green() {
    let repository = compiler_root().parent().unwrap();
    for (script, expected) in [
        (
            "check-step-6-artifact-instance.mjs",
            "1 target, 4 host rows, 30 managed files, 3 subordinate manifests, 24 mutation controls",
        ),
        (
            "check-step-6-instance-replay.mjs",
            "1 family, 4 hosts, 2 cases, 8 replay checks, 11 mutation controls",
        ),
    ] {
        let output = Command::new("node")
            .arg(compiler_root().join("lit/j57-2c").join(script))
            .current_dir(repository)
            .output()
            .expect("step-6 checker starts");
        assert!(output.status.success(), "{script}: {output:?}");
        assert!(output.stderr.is_empty(), "{script}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(expected),
            "{script}: {output:?}"
        );
    }
}

#[test]
fn frozen_instance_replay_validation_remains_green() {
    let repository = compiler_root().parent().unwrap();
    // The activated-instance closure checker also asserted that the mutable
    // current product tree was the superseded 5.7.0/topaz-5.6 candidate. Keep
    // only the immutable historical instance replay in the ordinary suite.
    let script = "check-step-7-instance-replay.mjs";
    let output = Command::new("node")
        .arg(compiler_root().join("lit/j57-2c").join(script))
        .current_dir(repository)
        .output()
        .expect("step-7 replay checker starts");
    assert!(output.status.success(), "{script}: {output:?}");
    assert!(output.stderr.is_empty(), "{script}: {output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("1 family, 4 hosts, 2 cases, 8 replay checks, 11 mutation controls"),
        "{script}: {output:?}"
    );
}

#[test]
fn historical_h5_array_mode_remains_a_separate_branch() {
    let request =
        std::fs::read(compiler_root().join("lit/h5/fixtures/000-differential_begin-nested.json"))
            .unwrap();
    let output = run_lit(&request);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(
        output
            .stdout
            .starts_with(b"[\"lispex.hosted-source-backend-result/v0\"")
    );
}
