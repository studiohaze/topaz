use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::rc::Rc;

use topaz_value::value::{JsonValue, json_parse, sha256};

const MANIFEST_PATH: &str = "share/topaz/lispex/lit-artifact-manifest.v1.json";
const PROFILE: &str = "lispex-profile-1.5";
const ABI: &str = "lispex-lit-abi/v1";
const ARTIFACT_IDS: [&str; 4] = [
    "topaz-interpreter",
    "generated-rust",
    "generated-python",
    "web",
];
const NATIVE_ARTIFACT_INDEX: usize = 1;
const NATIVE_HOST_VARIANT: &str = ARTIFACT_IDS[NATIVE_ARTIFACT_INDEX];
const PRODUCT_PROFILE: &str = "lispex-product-native/v1";
const PRODUCT_LIMITS: [u64; 5] = [20_000_000, 2_000_000, 20_000_000, 100_000_000, 16_777_216];
const MAXIMUM_TRANSPORT_BYTES: usize = 32 * 1024 * 1024;
const FALLBACK_KEYS: [&str; 9] = [
    "debug_binary",
    "host_apply",
    "host_callback",
    "host_control",
    "host_eval",
    "host_source_decoder",
    "runtime_download",
    "rust_backend",
    "sibling_checkout",
];

type Object = BTreeMap<Rc<str>, JsonValue>;

#[derive(Debug, PartialEq, Eq)]
struct Identity {
    identity_kind: &'static str,
    path: String,
    byte_len: u64,
    sha256: String,
}

#[derive(Debug)]
struct InstalledLit {
    root: PathBuf,
    language_mode: String,
    target: String,
    host_variant: String,
    canonical_source: Identity,
    artifacts: [Identity; ARTIFACT_IDS.len()],
    native_executable: Identity,
}

impl InstalledLit {
    fn native_artifact(&self) -> &Identity {
        &self.artifacts[NATIVE_ARTIFACT_INDEX]
    }
}

pub(crate) fn info_json() -> ExitCode {
    match discover_and_validate() {
        Ok(installed) => {
            let artifacts = installed
                .artifacts
                .iter()
                .enumerate()
                .map(|(index, identity)| {
                    format!(
                        "{{\"id\":{},\"identity\":{}}}",
                        json_string(ARTIFACT_IDS[index]),
                        render_identity(identity)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            println!(
                "{{\"schema\":\"topaz.lispex-info/v1\",\"available\":true,\"topaz_version\":{},\"language_mode\":{},\"lispex_profile\":{},\"abi\":{},\"target\":{},\"host_variant\":{},\"canonical_source\":{},\"artifacts\":[{}]}}",
                json_string(env!("CARGO_PKG_VERSION")),
                json_string(&installed.language_mode),
                json_string(PROFILE),
                json_string(ABI),
                json_string(&installed.target),
                json_string(&installed.host_variant),
                render_identity(&installed.canonical_source),
                artifacts,
            );
            ExitCode::SUCCESS
        }
        Err(error) => product_error(&error),
    }
}

pub(crate) fn run_program(source_path: &str) -> ExitCode {
    let installed = match discover_and_validate() {
        Ok(value) => value,
        Err(error) => return product_error(&error),
    };
    let source = match fs::read(source_path) {
        Ok(value) => value,
        Err(error) => return product_error(&format!("cannot read `{source_path}`: {error}")),
    };
    let logical_name = Path::new(source_path)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| neutral_component(value))
        .unwrap_or("program.lspx");
    let logical_source_id = format!("cli/{logical_name}");
    let source_sha = sha256_hex(&source);
    let request = format!(
        "{{\"schema\":\"lispex.backend-run-request/v1\",\"invocation_id\":\"topaz-lispex-cli\",\"profile\":\"{PROFILE}\",\"logical_source_id\":{},\"source_bytes_base64\":{},\"source_sha256\":{},\"resource_profile\":\"{}\",\"limits\":[{},{},{},{},{}],\"artifact_expectation\":{{\"backend_id\":\"lit\",\"profile\":\"{PROFILE}\",\"capability_schema\":\"lispex.primitive-capabilities/v2\",\"observation_schema\":\"lispex.observation-result/v1\",\"artifact\":{}}},\"cancellation\":{{\"deadline_unix_millis\":null,\"abort_requested\":false}}}}\n",
        json_string(&logical_source_id),
        json_string(&base64_encode(&source)),
        json_string(&source_sha),
        PRODUCT_PROFILE,
        PRODUCT_LIMITS[0],
        PRODUCT_LIMITS[1],
        PRODUCT_LIMITS[2],
        PRODUCT_LIMITS[3],
        PRODUCT_LIMITS[4],
        render_identity(installed.native_artifact()),
    );
    let frame = format!(
        "{{\"schema\":\"topaz.lit-host-frame/v1\",\"request_jsonl\":{},\"binding\":{{\"schema\":\"topaz.lit-host-binding/v1\",\"host_variant\":\"{NATIVE_HOST_VARIANT}\",\"artifact\":{},\"deadline_unix_millis\":null,\"deadline_state\":\"none\",\"binary_stdio\":true,\"fresh_process\":true,\"process_control\":true}}}}\n",
        json_string(&request),
        render_identity(installed.native_artifact()),
    );
    let runner = match checked_path(&installed.root, &installed.native_executable.path) {
        Ok(value) => value,
        Err(error) => return product_error(&error),
    };
    let mut command = Command::new(runner);
    command
        .current_dir(&installed.root)
        .env_clear()
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut child = match command.spawn() {
        Ok(value) => value,
        Err(error) => return product_error(&format!("cannot start installed LIT runner: {error}")),
    };
    if let Err(error) = child
        .stdin
        .as_mut()
        .expect("installed LIT stdin is piped")
        .write_all(frame.as_bytes())
    {
        return product_error(&format!("cannot write installed LIT request: {error}"));
    }
    let output = match child.wait_with_output() {
        Ok(value) => value,
        Err(error) => return product_error(&format!("installed LIT runner wait failed: {error}")),
    };
    if !output.status.success() || !output.stderr.is_empty() {
        return product_error(
            "installed LIT runner violated the zero-status/empty-stderr transport contract",
        );
    }
    if output.stdout.len() > MAXIMUM_TRANSPORT_BYTES {
        return product_error("installed LIT runner exceeded the transport byte limit");
    }
    if output.stdout.last() != Some(&b'\n')
        || output.stdout[..output.stdout.len().saturating_sub(1)].contains(&b'\n')
        || output.stdout[..output.stdout.len().saturating_sub(1)].contains(&b'\r')
    {
        return product_error("installed LIT runner returned invalid JSONL framing");
    }
    let observation = match std::str::from_utf8(&output.stdout)
        .ok()
        .and_then(|text| json_parse(text).ok())
    {
        Some(value) => value,
        None => return product_error("installed LIT runner returned invalid UTF-8 JSON"),
    };
    match validate_and_project_observation(
        &observation,
        &installed,
        &source_sha,
        "topaz-lispex-cli",
    ) {
        Ok(code) => code,
        Err(error) => product_error(&error),
    }
}

fn discover_and_validate() -> Result<InstalledLit, String> {
    let product_version = env!("CARGO_PKG_VERSION");
    let language_mode = format!(
        "topaz-{}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR")
    );
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot resolve current executable: {error}"))?;
    let bin = executable
        .parent()
        .ok_or_else(|| "current executable has no bin directory".to_string())?;
    if bin.file_name().and_then(|value| value.to_str()) != Some("bin") {
        return Err("Topaz executable is not in the required install-root bin directory".into());
    }
    let root = fs::canonicalize(
        bin.parent()
            .ok_or_else(|| "Topaz bin directory has no install root".to_string())?,
    )
    .map_err(|error| format!("cannot canonicalize install root: {error}"))?;
    let manifest_path = checked_path(&root, MANIFEST_PATH)?;
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|error| format!("cannot read installed LIT manifest: {error}"))?;
    let manifest = std::str::from_utf8(&manifest_bytes)
        .ok()
        .and_then(|text| json_parse(text).ok())
        .ok_or_else(|| "installed LIT manifest is not valid UTF-8 JSON".to_string())?;
    let object = as_object(&manifest, "manifest")?;
    expect_text(object, "schema", "topaz.lit-artifact-manifest/v1")?;
    expect_text(object, "instance_schema", "topaz.lit-artifact-instance/v1")?;
    expect_text(object, "checkpoint", "J57-2C-step-7")?;
    expect_text(object, "status", "activated-current")?;
    expect_text(object, "profile", PROFILE)?;
    expect_text(object, "abi", ABI)?;
    let availability = object_field(object, "availability")?;
    let availability = as_object(availability, "availability")?;
    expect_bool(availability, "available", true)?;
    expect_bool(availability, "discoverable_by_product", true)?;

    let toolchain = as_object(object_field(object, "toolchain")?, "toolchain")?;
    for field in [
        "product_version",
        "minimum_version",
        "active_generator_version",
        "backend_version",
    ] {
        expect_text(toolchain, field, product_version)?;
    }
    expect_text(toolchain, "language_mode", &language_mode)?;
    let target = as_object(object_field(object, "target")?, "target")?;
    let product_target = text_field(target, "product_target")?.to_string();
    let host_variant = text_field(target, "host_variant")?.to_string();

    let managed = as_array(object_field(object, "managed_files")?, "managed_files")?;
    if managed.is_empty() {
        return Err("installed LIT manifest has no managed files".into());
    }
    let mut managed_paths = Vec::new();
    for value in managed {
        let row = as_object(value, "managed file")?;
        let identity = identity_from(row, "installed-managed-file-bytes")?;
        if managed_paths.iter().any(|path| path == &identity.path) {
            return Err(format!("duplicate managed path `{}`", identity.path));
        }
        let bytes = fs::read(checked_path(&root, &identity.path)?)
            .map_err(|error| format!("cannot read managed `{}`: {error}", identity.path))?;
        validate_identity(&identity, &bytes)?;
        managed_paths.push(identity.path);
    }
    if !managed_paths.iter().any(|path| path == "bin/topaz.exe") {
        return Err("installed manifest does not bind bin/topaz.exe".into());
    }
    let installed_executable = fs::canonicalize(&executable)
        .map_err(|error| format!("cannot canonicalize current executable: {error}"))?;
    if installed_executable != checked_path(&root, "bin/topaz.exe")? {
        return Err("running executable does not match installed bin/topaz.exe".into());
    }

    let canonical = as_object(
        object_field(object, "canonical_source")?,
        "canonical_source",
    )?;
    let canonical_source = identity_from(canonical, "topaz-lit-canonical-source-bytes")?;
    let source_bytes = fs::read(checked_path(&root, &canonical_source.path)?)
        .map_err(|error| format!("cannot read canonical LIT source: {error}"))?;
    validate_identity(&canonical_source, &source_bytes)?;

    let artifacts_value = as_array(object_field(object, "artifacts")?, "artifacts")?;
    let [interpreter_row, native_row, python_row, web_row] = artifacts_value else {
        return Err("installed LIT manifest must contain four artifact rows".into());
    };
    let interpreter_artifact = installed_artifact(&root, interpreter_row, ARTIFACT_IDS[0])?;
    let native_artifact = installed_artifact(&root, native_row, ARTIFACT_IDS[1])?;
    let native_executable = identity_from(
        as_object(
            object_field(as_object(native_row, "artifact row")?, "executable")?,
            "executable identity",
        )?,
        "executed-native-binary-bytes",
    )?;
    if native_executable.path != native_artifact.path
        || native_executable.byte_len != native_artifact.byte_len
        || native_executable.sha256 != native_artifact.sha256
    {
        return Err("native artifact/executable identity mismatch".into());
    }
    let python_artifact = installed_artifact(&root, python_row, ARTIFACT_IDS[2])?;
    let web_artifact = installed_artifact(&root, web_row, ARTIFACT_IDS[3])?;
    let artifacts = [
        interpreter_artifact,
        native_artifact,
        python_artifact,
        web_artifact,
    ];

    let fallbacks = as_object(
        object_field(object, "forbidden_fallback_counts")?,
        "fallbacks",
    )?;
    if fallbacks.len() != FALLBACK_KEYS.len() {
        return Err("installed LIT fallback map shape mismatch".into());
    }
    for key in FALLBACK_KEYS {
        if integer_field(fallbacks, key)? != 0 {
            return Err(format!("forbidden fallback `{key}` is nonzero"));
        }
    }
    let boundary = as_object(object_field(object, "claim_boundary")?, "claim boundary")?;
    expect_bool(boundary, "installed_product", true)?;
    expect_bool(boundary, "product_discovery", true)?;
    expect_bool(boundary, "version_activation", true)?;
    expect_bool(boundary, "public_packaging_or_release_change", false)?;
    expect_bool(boundary, "capability_promotion", false)?;

    Ok(InstalledLit {
        root,
        language_mode,
        target: product_target,
        host_variant,
        canonical_source,
        artifacts,
        native_executable,
    })
}

fn installed_artifact(
    root: &Path,
    value: &JsonValue,
    expected_id: &str,
) -> Result<Identity, String> {
    let row = as_object(value, "artifact row")?;
    expect_text(row, "id", expected_id)?;
    expect_text(row, "status", "activated-current")?;
    expect_text(row, "host_variant", expected_id)?;
    let artifact = identity_from(
        as_object(object_field(row, "artifact")?, "artifact identity")?,
        "topaz-lit-product-artifact-bytes",
    )?;
    let artifact_bytes = fs::read(checked_path(root, &artifact.path)?)
        .map_err(|error| format!("cannot read artifact `{}`: {error}", artifact.path))?;
    validate_identity(&artifact, &artifact_bytes)?;
    Ok(artifact)
}

fn validate_and_project_observation(
    value: &JsonValue,
    installed: &InstalledLit,
    source_sha: &str,
    invocation_id: &str,
) -> Result<ExitCode, String> {
    let object = as_object(value, "observation")?;
    expect_fields(
        object,
        &[
            "backend",
            "diagnostics",
            "exit_status",
            "identity",
            "invocation_id",
            "metrics",
            "resource_outcome",
            "schema",
            "status",
            "stdout_bytes",
            "value_projection",
            "warnings",
        ],
        "observation",
    )?;
    expect_text(object, "schema", "lispex.observation-result/v1")?;
    expect_text(object, "invocation_id", invocation_id)?;
    let backend = as_object(object_field(object, "backend")?, "backend")?;
    expect_fields(
        backend,
        &["host_variant", "id", "profile", "version"],
        "backend",
    )?;
    expect_text(backend, "id", "lit")?;
    expect_text(backend, "version", env!("CARGO_PKG_VERSION"))?;
    expect_text(backend, "profile", PROFILE)?;
    expect_text(backend, "host_variant", NATIVE_HOST_VARIANT)?;
    let identity = as_object(object_field(object, "identity")?, "identity")?;
    expect_fields(
        identity,
        &[
            "artifact",
            "capability_manifest_digest",
            "capability_manifest_id",
            "capability_schema",
            "forbidden_fallback_counts",
            "meter_id",
            "meter_manifest_digest",
            "observation_contract_digest",
            "observation_contract_id",
            "observation_schema",
            "observation_schema_id",
            "profile_id",
            "resource_schema",
            "source_sha256",
        ],
        "identity",
    )?;
    expect_text(identity, "source_sha256", source_sha)?;
    expect_text(
        identity,
        "capability_schema",
        "lispex.primitive-capabilities/v2",
    )?;
    expect_text(
        identity,
        "observation_schema",
        "lispex.observation-result/v1",
    )?;
    expect_text(identity, "profile_id", PROFILE)?;
    expect_text(identity, "meter_id", "lispex.product-resource-meter/v1")?;
    expect_text(
        identity,
        "capability_manifest_id",
        "lispex.capability-manifest/2",
    )?;
    expect_text(
        identity,
        "capability_manifest_digest",
        "sha256:22debb0569cabc51c8848d5a089e1b3c0b88e6c606525b92b5ce060e40f1d900",
    )?;
    expect_text(
        identity,
        "meter_manifest_digest",
        "sha256:0e7bfe9be2ea155d7f9d8b5f83f49b0df4451710891769b77423e1c7cecbf657",
    )?;
    expect_text(
        identity,
        "observation_schema_id",
        "lispex.observation-result/v1",
    )?;
    expect_text(
        identity,
        "observation_contract_id",
        "lispex-observation-contract/1",
    )?;
    expect_text(
        identity,
        "observation_contract_digest",
        "sha256:efb521008ae1077163b55bcb0f77e03fa586bd047ead2ee9b7f5fa25e4a6c7f8",
    )?;
    expect_text(identity, "resource_schema", "lispex.resource-profiles/v1")?;
    let returned_artifact = identity_from(
        as_object(object_field(identity, "artifact")?, "returned artifact")?,
        "topaz-lit-product-artifact-bytes",
    )?;
    if &returned_artifact != installed.native_artifact() {
        return Err("installed LIT observation artifact identity drift".into());
    }
    let fallbacks = as_object(
        object_field(identity, "forbidden_fallback_counts")?,
        "fallbacks",
    )?;
    if fallbacks.len() != FALLBACK_KEYS.len() {
        return Err("installed LIT observation fallback map shape mismatch".into());
    }
    for key in FALLBACK_KEYS {
        if integer_field(fallbacks, key)? != 0 {
            return Err(format!(
                "installed LIT observation used forbidden fallback `{key}`"
            ));
        }
    }
    let status = text_field(object, "status")?;
    if !matches!(
        status,
        "ok" | "source-error" | "runtime-error" | "resource" | "infrastructure-error"
    ) {
        return Err("installed LIT observation status is incompatible".into());
    }
    let stdout = byte_channel(object_field(object, "stdout_bytes")?)?;
    let warnings = diagnostic_channels(object_field(object, "warnings")?, true)?;
    let diagnostics = diagnostic_channels(object_field(object, "diagnostics")?, false)?;
    if status == "ok" && !diagnostics.is_empty() {
        return Err("successful installed LIT observation contains diagnostics".into());
    }
    if status != "ok" && diagnostics.is_empty() {
        return Err("failed installed LIT observation has no diagnostic".into());
    }
    let code = integer_field(object, "exit_status")?;
    let expected_code = match status {
        "ok" => 0,
        "source-error" | "runtime-error" => 1,
        "resource" | "infrastructure-error" => 2,
        _ => unreachable!(),
    };
    if code != expected_code {
        return Err("installed LIT observation status/exit mapping drift".into());
    }
    validate_value_projection(object_field(object, "value_projection")?)?;
    validate_resource_outcome(object_field(object, "resource_outcome")?, status, &stdout)?;
    let metrics = as_object(object_field(object, "metrics")?, "metrics")?;
    expect_bool(metrics, "protocol_admitted", true)?;
    expect_bool(metrics, "source_execution_started", true)?;
    expect_bool(
        metrics,
        "artifact_identity_verified_by_executing_host",
        true,
    )?;
    if integer_field(metrics, "host_utf8_source_decoder_count")? != 0
        || integer_field(metrics, "capability_row_count")? != 205
        || integer_field(metrics, "rust_supported_count")? != 205
        || integer_field(metrics, "lil_supported_count")? != 84
        || integer_field(metrics, "lit_supported_count")? != 84
    {
        return Err("installed LIT observation metric boundary drift".into());
    }

    std::io::stdout()
        .write_all(&stdout)
        .map_err(|error| format!("cannot write Lispex stdout: {error}"))?;
    let mut stderr = std::io::stderr();
    for rendered in warnings.iter().chain(diagnostics.iter()) {
        stderr
            .write_all(rendered)
            .map_err(|error| format!("cannot write Lispex diagnostic: {error}"))?;
    }
    Ok(ExitCode::from(code as u8))
}

fn diagnostic_channels(value: &JsonValue, warning: bool) -> Result<Vec<Vec<u8>>, String> {
    let mut channels = Vec::new();
    for value in as_array(value, "diagnostics")? {
        let diagnostic = as_object(value, "diagnostic")?;
        expect_fields(
            diagnostic,
            &[
                "code",
                "family",
                "irritants",
                "message",
                "phase",
                "rendered_bytes",
                "severity",
                "span",
            ],
            "diagnostic",
        )?;
        for field in ["code", "family", "message", "phase"] {
            let _ = text_field(diagnostic, field)?;
        }
        let severity = text_field(diagnostic, "severity")?;
        if warning != (severity == "warning") {
            return Err("installed LIT diagnostic severity/channel drift".into());
        }
        for irritant in as_array(object_field(diagnostic, "irritants")?, "irritants")? {
            if !matches!(irritant, JsonValue::String(_)) {
                return Err("installed LIT diagnostic irritant must be a string".into());
            }
        }
        let span = as_object(object_field(diagnostic, "span")?, "diagnostic span")?;
        expect_fields(span, &["column", "line", "source"], "diagnostic span")?;
        let _ = text_field(span, "source")?;
        if integer_field(span, "line")? <= 0 || integer_field(span, "column")? <= 0 {
            return Err("installed LIT diagnostic span is invalid".into());
        }
        channels.push(byte_channel(object_field(diagnostic, "rendered_bytes")?)?);
    }
    Ok(channels)
}

fn validate_value_projection(value: &JsonValue) -> Result<(), String> {
    let projection = as_object(value, "value projection")?;
    expect_fields(projection, &["schema", "values"], "value projection")?;
    expect_text(projection, "schema", "lispex.value-projection/v1")?;
    for value in as_array(object_field(projection, "values")?, "projected values")? {
        if !matches!(value, JsonValue::String(_)) {
            return Err("installed LIT projected value must be a string".into());
        }
    }
    Ok(())
}

fn validate_resource_outcome(value: &JsonValue, status: &str, stdout: &[u8]) -> Result<(), String> {
    let outcome = as_object(value, "resource outcome")?;
    expect_fields(
        outcome,
        &["limit", "limits", "profile_id", "purpose", "status"],
        "resource outcome",
    )?;
    expect_text(outcome, "profile_id", PRODUCT_PROFILE)?;
    expect_text(outcome, "purpose", "installed-product")?;
    let limits = as_object(object_field(outcome, "limits")?, "resource limits")?;
    let limit_fields = [
        "frontend_transition_limit",
        "token_limit",
        "normalization_step_limit",
        "machine_transition_limit",
        "output_byte_limit",
    ];
    expect_fields(limits, &limit_fields, "resource limits")?;
    for (field, expected) in limit_fields.iter().zip(PRODUCT_LIMITS) {
        if integer_field(limits, field)? != expected as i64 {
            return Err("installed LIT resource limits drift".into());
        }
    }
    let resource_status = text_field(outcome, "status")?;
    let limit = object_field(outcome, "limit")?;
    match status {
        "resource" => {
            if resource_status != "limit-exceeded"
                || !matches!(limit, JsonValue::String(_))
                || !stdout.is_empty()
            {
                return Err("installed LIT resource result is incompatible".into());
            }
        }
        "source-error" | "infrastructure-error" => {
            if resource_status != "not-started" || !matches!(limit, JsonValue::Null) {
                return Err("installed LIT pre-execution resource result drift".into());
            }
        }
        "ok" | "runtime-error" => {
            if resource_status != "within-limit" || !matches!(limit, JsonValue::Null) {
                return Err("installed LIT completed resource result drift".into());
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn checked_path(root: &Path, neutral: &str) -> Result<PathBuf, String> {
    if !neutral_path(neutral) {
        return Err(format!("installed LIT path is unsafe: `{neutral}`"));
    }
    let joined = root.join(neutral.replace('/', std::path::MAIN_SEPARATOR_STR));
    let canonical = fs::canonicalize(&joined)
        .map_err(|error| format!("cannot resolve installed `{neutral}`: {error}"))?;
    if !canonical.starts_with(root) {
        return Err(format!("installed LIT path escaped its root: `{neutral}`"));
    }
    Ok(canonical)
}

fn neutral_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
}

fn neutral_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.starts_with('/')
        && !value.contains('\\')
        && value.as_bytes().get(1).is_none_or(|value| *value != b':')
        && value.split('/').all(neutral_component)
}

fn identity_from(object: &Object, kind: &'static str) -> Result<Identity, String> {
    expect_text(object, "identity_kind", kind)?;
    let path = text_field(object, "path")?.to_string();
    if !neutral_path(&path) {
        return Err(format!("identity path is unsafe: `{path}`"));
    }
    let byte_len = integer_field(object, "byte_len")?;
    if byte_len <= 0 {
        return Err(format!("identity `{path}` has an invalid byte length"));
    }
    let digest = text_field(object, "sha256")?.to_string();
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!("identity `{path}` has an invalid SHA-256"));
    }
    Ok(Identity {
        identity_kind: kind,
        path,
        byte_len: byte_len as u64,
        sha256: digest,
    })
}

fn validate_identity(identity: &Identity, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 != identity.byte_len || sha256_hex(bytes) != identity.sha256 {
        return Err(format!("installed identity drift: `{}`", identity.path));
    }
    Ok(())
}

fn object_field<'a>(object: &'a Object, field: &str) -> Result<&'a JsonValue, String> {
    object
        .get(field)
        .ok_or_else(|| format!("installed JSON field `{field}` is missing"))
}

fn as_object<'a>(value: &'a JsonValue, context: &str) -> Result<&'a Object, String> {
    let JsonValue::Object(value) = value else {
        return Err(format!("installed {context} must be an object"));
    };
    Ok(value)
}

fn as_array<'a>(value: &'a JsonValue, context: &str) -> Result<&'a [JsonValue], String> {
    let JsonValue::Array(value) = value else {
        return Err(format!("installed {context} must be an array"));
    };
    Ok(value)
}

fn expect_fields(object: &Object, expected: &[&str], context: &str) -> Result<(), String> {
    if object.len() != expected.len() || expected.iter().any(|field| !object.contains_key(*field)) {
        return Err(format!("installed {context} fields are missing or unknown"));
    }
    Ok(())
}

fn text_field<'a>(object: &'a Object, field: &str) -> Result<&'a str, String> {
    let JsonValue::String(value) = object_field(object, field)? else {
        return Err(format!("installed JSON field `{field}` must be a string"));
    };
    Ok(value)
}

fn integer_field(object: &Object, field: &str) -> Result<i64, String> {
    let JsonValue::Number(value) = object_field(object, field)? else {
        return Err(format!("installed JSON field `{field}` must be an integer"));
    };
    value
        .int
        .ok_or_else(|| format!("installed JSON field `{field}` must be an exact integer"))
}

fn expect_text(object: &Object, field: &str, expected: &str) -> Result<(), String> {
    if text_field(object, field)? != expected {
        return Err(format!("installed JSON field `{field}` is incompatible"));
    }
    Ok(())
}

fn expect_bool(object: &Object, field: &str, expected: bool) -> Result<(), String> {
    let JsonValue::Bool(value) = object_field(object, field)? else {
        return Err(format!("installed JSON field `{field}` must be bool"));
    };
    if *value != expected {
        return Err(format!("installed JSON field `{field}` is incompatible"));
    }
    Ok(())
}

fn byte_channel(value: &JsonValue) -> Result<Vec<u8>, String> {
    let object = as_object(value, "byte channel")?;
    expect_text(object, "encoding", "base64")?;
    let bytes = base64_decode(text_field(object, "base64")?)?;
    if integer_field(object, "byte_len")? != bytes.len() as i64
        || text_field(object, "sha256")? != sha256_hex(&bytes)
    {
        return Err("installed LIT byte channel identity drift".into());
    }
    Ok(bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = sha256(bytes);
    let mut output = String::with_capacity(64);
    topaz_value::bytes_to_hex_into(&mut output, &digest);
    output
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        output.push(ALPHABET[((value >> 18) & 63) as usize] as char);
        output.push(ALPHABET[((value >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(4) || !text.is_ascii() {
        return Err("installed LIT byte channel has invalid base64 length".into());
    }
    fn digit(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let input = text.as_bytes();
    let mut output = Vec::with_capacity(input.len() / 4 * 3);
    for (index, chunk) in input.chunks_exact(4).enumerate() {
        let last = index + 1 == input.len() / 4;
        let padding = usize::from(chunk[3] == b'=') + usize::from(chunk[2] == b'=');
        if padding > 0 && !last || padding > 2 || (chunk[2] == b'=' && chunk[3] != b'=') {
            return Err("installed LIT byte channel has invalid base64 padding".into());
        }
        let a = digit(chunk[0])
            .ok_or_else(|| "installed LIT byte channel has invalid base64".to_string())?;
        let b = digit(chunk[1])
            .ok_or_else(|| "installed LIT byte channel has invalid base64".to_string())?;
        let c = if chunk[2] == b'=' {
            0
        } else {
            digit(chunk[2])
                .ok_or_else(|| "installed LIT byte channel has invalid base64".to_string())?
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            digit(chunk[3])
                .ok_or_else(|| "installed LIT byte channel has invalid base64".to_string())?
        };
        let value =
            (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6) | u32::from(d);
        output.push((value >> 16) as u8);
        if padding < 2 {
            output.push((value >> 8) as u8);
        }
        if padding < 1 {
            output.push(value as u8);
        }
    }
    Ok(output)
}

fn render_identity(identity: &Identity) -> String {
    format!(
        "{{\"identity_kind\":{},\"path\":{},\"byte_len\":{},\"sha256\":{}}}",
        json_string(identity.identity_kind),
        json_string(&identity.path),
        identity.byte_len,
        json_string(&identity.sha256),
    )
}

fn json_string(value: &str) -> String {
    let mut output = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            value if value <= '\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(output, "\\u{:04x}", u32::from(value));
            }
            value => output.push(value),
        }
    }
    output.push('"');
    output
}

fn product_error(message: &str) -> ExitCode {
    eprintln!("topaz: Lispex component unavailable: {message}");
    ExitCode::FAILURE
}
