use crate::*;

#[derive(Clone, Copy)]
pub(super) enum HostHarness<'a> {
    Unrestricted,
    PackageFs {
        read_roots: &'a [String],
        write_roots: &'a [String],
        extern_replay_jsonl: &'a str,
        extern_sandbox_policies: &'a [topaz_value::ExternSandboxPolicy],
    },
    PackageFsLispex {
        read_roots: &'a [String],
        write_roots: &'a [String],
        extern_replay_jsonl: &'a str,
        extern_sandbox_policies: &'a [topaz_value::ExternSandboxPolicy],
        plan: &'a topaz_lispex_product::CheckedApplicationPlan,
    },
}

impl<'a> HostHarness<'a> {
    pub(super) fn lispex_application(
        self,
    ) -> Option<&'a topaz_lispex_product::CheckedApplicationPlan> {
        match self {
            Self::PackageFsLispex { plan, .. } => Some(plan),
            Self::Unrestricted | Self::PackageFs { .. } => None,
        }
    }
}

pub(super) fn package_harness(target: &PackageTarget) -> HostHarness<'_> {
    HostHarness::PackageFs {
        read_roots: &target.fs_read_roots,
        write_roots: &target.fs_write_roots,
        extern_replay_jsonl: &target.extern_replay_jsonl,
        extern_sandbox_policies: &target.extern_sandbox_policies,
    }
}

pub(super) fn package_harness_with_lispex<'a>(
    target: &'a PackageTarget,
    plan: Option<&'a topaz_lispex_product::CheckedApplicationPlan>,
) -> HostHarness<'a> {
    match plan.filter(|plan| !plan.reachable_rules.is_empty()) {
        Some(plan) => HostHarness::PackageFsLispex {
            read_roots: &target.fs_read_roots,
            write_roots: &target.fs_write_roots,
            extern_replay_jsonl: &target.extern_replay_jsonl,
            extern_sandbox_policies: &target.extern_sandbox_policies,
            plan,
        },
        None => package_harness(target),
    }
}

/// The `src/main.rs` host harness of a scaffolded crate: it runs the emitted
/// `run_with_host` on a `NativeHost` and maps the structured outcome to the
/// process exit code (a fault prints its message to stderr and exits non-zero).
pub(super) fn host_init(harness: HostHarness<'_>) -> String {
    match harness {
        HostHarness::Unrestricted => "topaz_host_native::NativeHost::new()".to_string(),
        HostHarness::PackageFs {
            read_roots,
            write_roots,
            extern_replay_jsonl,
            extern_sandbox_policies,
        } => package_fs_host_init(
            read_roots,
            write_roots,
            extern_replay_jsonl,
            extern_sandbox_policies,
        ),
        HostHarness::PackageFsLispex {
            read_roots,
            write_roots,
            extern_replay_jsonl,
            extern_sandbox_policies,
            plan,
        } => {
            let inner = package_fs_host_init(
                read_roots,
                write_roots,
                extern_replay_jsonl,
                extern_sandbox_policies,
            );
            lispex_application_host_init(&inner, plan)
        }
    }
}

pub(super) fn package_fs_host_init(
    read_roots: &[String],
    write_roots: &[String],
    extern_replay_jsonl: &str,
    extern_sandbox_policies: &[topaz_value::ExternSandboxPolicy],
) -> String {
    let read_roots = rust_string_vec_literal(read_roots);
    let write_roots = rust_string_vec_literal(write_roots);
    let extern_replay_jsonl = rust_string_literal(extern_replay_jsonl);
    let extern_sandbox_policies = rust_extern_sandbox_policy_vec_literal(extern_sandbox_policies);
    let mut init = String::new();
    init.push_str("{\n");
    init.push_str(
        r#"    let runtime_root = match std::env::current_dir() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("cannot determine application runtime directory: {error}");
            return ExitCode::FAILURE;
        }
    };
"#,
    );
    init.push_str(&format!("    let fs_read_roots = {read_roots};\n"));
    init.push_str(&format!("    let fs_write_roots = {write_roots};\n"));
    init.push_str(&format!(
        "    let extern_sandbox_policies = {extern_sandbox_policies};\n"
    ));
    init.push_str(&format!(
                "    let extern_replay = topaz_rt::ExternReplayStore::parse_jsonl_with_policies({extern_replay_jsonl}, extern_sandbox_policies)\n"
            ));
    init.push_str(
        "        .expect(\"embedded extern replay fixture was validated before emit\");\n",
    );
    init.push_str("    topaz_host_native::NativeHost::with_fs_capabilities(\n");
    init.push_str("        &runtime_root,\n");
    init.push_str("        &fs_read_roots,\n");
    init.push_str("        &fs_write_roots,\n");
    init.push_str("    )\n");
    init.push_str("    .with_extern_replay(extern_replay)\n");
    init.push('}');
    init
}

pub(super) fn lispex_application_host_init(
    inner: &str,
    plan: &topaz_lispex_product::CheckedApplicationPlan,
) -> String {
    let mut init = String::from("{\n");
    init.push_str("    let inner = ");
    init.push_str(inner);
    init.push_str(";\n    let rules = vec![\n");
    for rule in &plan.rules {
        let payload_path = plan
            .payload
            .files
            .iter()
            .find(|file| {
                file.path.starts_with("lispex/rules/") && file.bytes == rule.prepared_artifact
            })
            .expect("checked plan carries each selected prepared artifact")
            .path
            .replace('\\', "/");
        init.push_str(
            "        topaz_lispex_embed::AdmittedApplicationRule::from_locked_artifact(\n",
        );
        init.push_str("            topaz_lispex_embed::LispexApplicationRuleIdentity {\n");
        for (field, value) in [
            ("name", &rule.identity.name),
            ("profile", &rule.identity.profile),
            ("component_id", &rule.identity.component_id),
            ("evaluator_sha256", &rule.identity.evaluator_sha256),
            (
                "prepared_artifact_sha256",
                &rule.identity.prepared_artifact_sha256,
            ),
            (
                "preparation_request_sha256",
                &rule.identity.preparation_request_sha256,
            ),
        ] {
            init.push_str(&format!(
                "                {field}: String::from({}),\n",
                rust_string_literal(value)
            ));
        }
        init.push_str("            },\n");
        init.push_str(&format!(
            "            {},\n",
            rust_string_literal(&rule.preparation_submission_sha256)
        ));
        init.push_str(&format!(
            "            include_bytes!({}).as_slice(),\n",
            rust_string_literal(&format!("../{payload_path}"))
        ));
        init.push_str(&format!(
            "            topaz_lispex_embed::EvaluateLimits {{ canonical_input_bytes: {}, eval_work: {}, logical_allocation: {}, semantic_frames: {}, traversal_depth: {}, output_bytes: {}, diagnostic_bytes: {}, transcript_bytes: {}, transcript_events: {}, result_bytes: {} }},\n",
            rule.limits.canonical_input_bytes,
            rule.limits.eval_work,
            rule.limits.logical_allocation,
            rule.limits.semantic_frames,
            rule.limits.traversal_depth,
            rule.limits.output_bytes,
            rule.limits.diagnostic_bytes,
            rule.limits.transcript_bytes,
            rule.limits.transcript_events,
            rule.limits.result_bytes,
        ));
        init.push_str(
            "        ).expect(\"locked Lispex rule was verified before product generation\"),\n",
        );
    }
    init.push_str("    ];\n");
    let quotas = plan.quotas;
    init.push_str(&format!(
        "    let quotas = topaz_lispex_embed::ApplicationQuotas {{ concurrent_evaluations: {}, queued_evaluations: {}, total_evaluations: {}, aggregate_input_bytes: {}, aggregate_result_bytes: {}, aggregate_output_bytes: {}, aggregate_transcript_bytes: {}, aggregate_safety_fuel: {}, prepared_bytes: {}, wall_millis: {} }};\n",
        quotas.concurrent_evaluations,
        quotas.queued_evaluations,
        quotas.total_evaluations,
        quotas.aggregate_input_bytes,
        quotas.aggregate_result_bytes,
        quotas.aggregate_output_bytes,
        quotas.aggregate_transcript_bytes,
        quotas.aggregate_safety_fuel,
        quotas.prepared_bytes,
        quotas.wall_millis,
    ));
    init.push_str("    topaz_lispex_embed::LispexApplicationHost::new(inner, rules, quotas)\n");
    init.push_str(
        "        .expect(\"checked Lispex application plan must construct its exact host\")\n",
    );
    init.push('}');
    init
}

pub(super) fn main_harness(harness: HostHarness<'_>) -> String {
    let host_init = host_init(harness);
    "\
use std::process::ExitCode;
use std::rc::Rc;
use topaz_rt::Value;
use topaz_emitted as emitted;

fn main() -> ExitCode {
    let host_impl = __HOST_INIT__;
    let outcome = if emitted::TOPAZ_EXPLICIT_MAIN {
        let stdin = topaz_rt::Host::input(&host_impl);
        let args = std::env::args().skip(1).collect();
        let host: Rc<dyn topaz_rt::Host> = Rc::new(host_impl);
        emitted::run_with_host_and_input(host, args, stdin)
    } else {
        let host: Rc<dyn topaz_rt::Host> = Rc::new(host_impl);
        emitted::run_with_host(host)
    };
    match outcome {
        topaz_rt::RunOutcome::Completed(value) if emitted::TOPAZ_EXPLICIT_MAIN => {
            explicit_main_exit(value)
        }
        topaz_rt::RunOutcome::Completed(_) => ExitCode::SUCCESS,
        topaz_rt::RunOutcome::Faulted(err) => {
            eprintln!(\"topaz fault: {}\", err.message);
            ExitCode::FAILURE
        }
    }
}

fn explicit_main_exit(value: Value) -> ExitCode {
    match value {
        Value::Ok(inner) => match inner.as_ref() {
            Value::Int(code) if (0..=255).contains(code) => ExitCode::from(*code as u8),
            Value::Int(code) => {
                eprintln!(\"topaz: explicit main returned exit code {code}; expected 0..255\");
                ExitCode::FAILURE
            }
            other => {
                eprintln!(
                    \"topaz: explicit main returned `Ok({})`; expected `Ok(int)`\",
                    other.kind()
                );
                ExitCode::FAILURE
            }
        },
        Value::Err(inner) => match inner.as_ref() {
            Value::Str(message) => {
                eprintln!(\"{message}\");
                ExitCode::FAILURE
            }
            other => {
                eprintln!(
                    \"topaz: explicit main returned `Err({})`; expected `Err(string)`\",
                    other.kind()
                );
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!(
                \"topaz: explicit main returned `{}`; expected `Result<int, string>`\",
                other.kind()
            );
            ExitCode::FAILURE
        }
    }
}
"
    .replace("__HOST_INIT__", &host_init)
}

pub(super) fn service_host_function(harness: HostHarness<'_>) -> String {
    match harness {
        HostHarness::Unrestricted => {
            "fn new_topaz_host() -> Result<topaz_host_native::NativeHost, String> {\n    Ok(topaz_host_native::NativeHost::new())\n}\n"
                .to_string()
        }
        HostHarness::PackageFs {
            read_roots,
            write_roots,
            extern_replay_jsonl,
            extern_sandbox_policies,
        } => {
            let read_roots = rust_string_vec_literal(read_roots);
            let write_roots = rust_string_vec_literal(write_roots);
            let replay = rust_string_literal(extern_replay_jsonl);
            let policies = rust_extern_sandbox_policy_vec_literal(extern_sandbox_policies);
            format!(
                "fn new_topaz_host() -> Result<topaz_host_native::NativeHost, String> {{\n\
                 \x20   let runtime_root = std::env::current_dir().map_err(|error| format!(\"cannot determine application runtime directory: {{error}}\"))?;\n\
                 \x20   let fs_read_roots = {read_roots};\n\
                 \x20   let fs_write_roots = {write_roots};\n\
                 \x20   let policies = {policies};\n\
                 \x20   let replay = topaz_rt::ExternReplayStore::parse_jsonl_with_policies({replay}, policies)\n\
                 \x20       .map_err(|error| format!(\"invalid embedded extern replay fixture: {{error}}\"))?;\n\
                 \x20   Ok(topaz_host_native::NativeHost::with_fs_capabilities(\n\
                 \x20       &runtime_root, &fs_read_roots, &fs_write_roots\n\
                 \x20   ).with_extern_replay(replay))\n\
                 }}\n"
            )
        }
        HostHarness::PackageFsLispex { .. } => {
            unreachable!("Lispex application reachability must be refused before service output")
        }
    }
}

pub(super) fn service_main_harness(
    harness: HostHarness<'_>,
    config: &topaz_package::ServiceConfig,
) -> String {
    let host_function = service_host_function(harness);
    let bind = if config.bind == "::1" {
        "std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)"
    } else {
        "std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)"
    };
    let log_format = match config.log_format {
        topaz_package::ServiceLogFormat::Text => "topaz_host_http::LogFormat::Text",
        topaz_package::ServiceLogFormat::Json => "topaz_host_http::LogFormat::Json",
        topaz_package::ServiceLogFormat::Off => "topaz_host_http::LogFormat::Off",
    };
    r###"use std::cell::RefCell;
use std::collections::BTreeMap;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;
use topaz_host_http::{Handler, HandlerFault, HandlerFaultKind, HostConfig, OwnedRequest, OwnedResponse};
use topaz_rt::{Host, OrderedMap, Value, url_value};
use topaz_emitted as emitted;

__HOST_FUNCTION__

const SERVICE_HELP: &str = "Topaz bounded HTTP service\n\nUSAGE:\n    program [OPTIONS]\n\nOPTIONS:\n    --print-config\n    --bind <ip>\n    --port <1..65535>\n    --workers <1..64>\n    --max-connections <1..4096>\n    --queue-capacity <0..4096>\n    --max-target-bytes <256..16384>\n    --max-header-bytes <1024..65536>\n    --max-headers <1..128>\n    --max-body-bytes <0..16777216>\n    --header-timeout-ms <100..60000>\n    --body-timeout-ms <100..60000>\n    --handler-timeout-ms <10..60000>\n    --shutdown-grace-ms <0..60000>\n    --log-format <text|json|off>\n";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.as_slice() == ["--help"] || args.as_slice() == ["-h"] {
        print!("{SERVICE_HELP}");
        return ExitCode::SUCCESS;
    }
    let print_config_count = args.iter().filter(|arg| arg.as_str() == "--print-config").count();
    if print_config_count > 1 {
        eprintln!("topaz-service: duplicate service option `--print-config`");
        return ExitCode::FAILURE;
    }
    let print_config = print_config_count == 1;
    args.retain(|arg| arg != "--print-config");
    let defaults = HostConfig {
        bind: __BIND__,
        port: __PORT__,
        workers: __WORKERS__,
        max_connections: __MAX_CONNECTIONS__,
        queue_capacity: __QUEUE_CAPACITY__,
        max_target_bytes: __MAX_TARGET_BYTES__,
        max_header_bytes: __MAX_HEADER_BYTES__,
        max_headers: __MAX_HEADERS__,
        max_body_bytes: __MAX_BODY_BYTES__,
        header_timeout: Duration::from_millis(__HEADER_TIMEOUT_MS__),
        body_timeout: Duration::from_millis(__BODY_TIMEOUT_MS__),
        handler_timeout: Duration::from_millis(__HANDLER_TIMEOUT_MS__),
        shutdown_grace: Duration::from_millis(__SHUTDOWN_GRACE_MS__),
        log_format: __LOG_FORMAT__,
    };
    let config = match defaults.with_args(args) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("topaz-service: {error}");
            return ExitCode::FAILURE;
        }
    };
    if print_config {
        match config.effective_json() {
            Ok(json) => {
                print!("{json}");
                return ExitCode::SUCCESS;
            }
            Err(error) => {
                eprintln!("topaz-service: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    let max_response_bytes = config.max_body_bytes;
    let handler: Rc<dyn Handler> = Rc::new(move |request: OwnedRequest| async move {
        let request = request_value(request)?;
        let host_impl = new_topaz_host()
            .map_err(|error| HandlerFault::new(HandlerFaultKind::Runtime, error))?;
        let host: Rc<dyn Host> = Rc::new(host_impl);
        match emitted::call_export_with_host_future(host, "handle", vec![request]).await {
            Ok(value) => response_value(value, max_response_bytes),
            Err(error) => Err(HandlerFault::new(
                HandlerFaultKind::Runtime,
                format!("{}: {}", error.code, error.message),
            )),
        }
    });
    match topaz_host_http::serve(config, handler) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("topaz-service: {error}");
            ExitCode::FAILURE
        }
    }
}

fn request_value(request: OwnedRequest) -> Result<Value, HandlerFault> {
    let absolute = format!("http://{}{}", request.authority, request.target);
    let url = url_value(&absolute).map_err(|error| {
        HandlerFault::new(HandlerFaultKind::BadRequest, format!("invalid request URL: {error}"))
    })?;
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, value) in request.headers {
        let value = String::from_utf8(value).map_err(|_| {
            HandlerFault::new(
                HandlerFaultKind::BadRequest,
                format!("request header `{name}` is not UTF-8"),
            )
        })?;
        grouped.entry(name.to_ascii_lowercase()).or_default().push(value);
    }
    let mut headers = OrderedMap::new();
    for (name, values) in grouped {
        headers
            .insert_value(
                &Value::str(name),
                Value::array(values.into_iter().map(Value::str).collect()),
            )
            .map_err(|error| {
                HandlerFault::new(
                    HandlerFaultKind::BadRequest,
                    format!("cannot construct request headers: {error:?}"),
                )
            })?;
    }
    Ok(Value::nominal_record(
        "std.http.HttpRequest",
        [
            (Rc::from("method"), Value::str(request.method)),
            (Rc::from("url"), url),
            (
                Rc::from("headers"),
                Value::Map(Rc::new(RefCell::new(headers))),
            ),
            (
                Rc::from("body"),
                Value::Bytes(Rc::from(request.body.into_boxed_slice())),
            ),
        ],
    ))
}

fn response_value(value: Value, max_body_bytes: usize) -> Result<OwnedResponse, HandlerFault> {
    let Value::NominalRecord { record_id, fields, .. } = value else {
        return Err(invalid_response("handler did not return HttpResponse"));
    };
    if record_id.as_ref() != "HttpResponse" {
        return Err(invalid_response("handler returned a different nominal record"));
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field_name, _)| field_name.as_ref() == name)
            .map(|(_, value)| value)
    };
    let status = match field("status") {
        Some(Value::Int(status)) if (100..=599).contains(status) => *status as u16,
        _ => return Err(invalid_response("HttpResponse.status must be in 100..=599")),
    };
    let headers = response_headers(
        field("headers").ok_or_else(|| invalid_response("HttpResponse.headers is missing"))?,
    )?;
    let body = match field("body") {
        Some(Value::Bytes(body)) if body.len() <= max_body_bytes => body.to_vec(),
        Some(Value::Bytes(_)) => return Err(invalid_response("HttpResponse.body exceeds max-body-bytes")),
        _ => return Err(invalid_response("HttpResponse.body must be Bytes")),
    };
    Ok(OwnedResponse { status, headers, body })
}

fn response_headers(value: &Value) -> Result<Vec<(String, Vec<u8>)>, HandlerFault> {
    let Value::Map(headers) = value else {
        return Err(invalid_response("HttpResponse.headers must be Map<string, Array<string>>"));
    };
    let pairs = headers.borrow().pairs();
    let mut output = Vec::new();
    for (name, values) in pairs {
        let Value::Str(name) = name else {
            return Err(invalid_response("HttpResponse header name must be string"));
        };
        let Value::Array(values) = values else {
            return Err(invalid_response("HttpResponse header values must be Array<string>"));
        };
        for value in values.borrow().iter() {
            let Value::Str(value) = value else {
                return Err(invalid_response("HttpResponse header value must be string"));
            };
            output.push((name.to_string(), value.as_bytes().to_vec()));
        }
    }
    Ok(output)
}

fn invalid_response(message: impl Into<String>) -> HandlerFault {
    HandlerFault::new(HandlerFaultKind::InvalidResponse, message)
}
"###
        .replace("__HOST_FUNCTION__", &host_function)
        .replace("__BIND__", bind)
        .replace("__PORT__", &config.port.to_string())
        .replace("__WORKERS__", &config.workers.to_string())
        .replace("__MAX_CONNECTIONS__", &config.max_connections.to_string())
        .replace("__QUEUE_CAPACITY__", &config.queue_capacity.to_string())
        .replace("__MAX_TARGET_BYTES__", &config.max_target_bytes.to_string())
        .replace("__MAX_HEADER_BYTES__", &config.max_header_bytes.to_string())
        .replace("__MAX_HEADERS__", &config.max_headers.to_string())
        .replace("__MAX_BODY_BYTES__", &config.max_body_bytes.to_string())
        .replace("__HEADER_TIMEOUT_MS__", &config.header_timeout_ms.to_string())
        .replace("__BODY_TIMEOUT_MS__", &config.body_timeout_ms.to_string())
        .replace("__HANDLER_TIMEOUT_MS__", &config.handler_timeout_ms.to_string())
        .replace("__SHUTDOWN_GRACE_MS__", &config.shutdown_grace_ms.to_string())
        .replace("__LOG_FORMAT__", log_format)
}

pub(super) fn scaffold_service_crate(
    out_dir: &Path,
    rust: &str,
    harness: HostHarness<'_>,
    config: &topaz_package::ServiceConfig,
) -> std::io::Result<()> {
    scaffold_native_crate(out_dir, rust, harness, true)?;
    fs::write(
        out_dir.join("src/main.rs"),
        service_main_harness(harness, config),
    )
}
