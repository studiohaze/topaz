use std::fs::File;
use std::future::Future;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rmcp::{
    ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, ErrorData, Implementation, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
    },
    service::{RequestContext, RoleServer},
    service::{RxJsonRpcMessage, TxJsonRpcMessage},
    transport::{IntoTransport, Transport, stdio},
};
use serde::Deserialize;
use serde_json::{Map as JsonObject, Value as JsonValue, json};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

mod execution;

use execution::ExecutionRuntime;

struct ShutdownAwareTransport<T> {
    inner: T,
    shutdown: CancellationToken,
}

impl<T> Transport<RoleServer> for ShutdownAwareTransport<T>
where
    T: Transport<RoleServer>,
{
    type Error = T::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.inner.send(item)
    }

    fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleServer>>> + Send {
        let receive = self.inner.receive();
        let shutdown = self.shutdown.clone();
        async move {
            let message = receive.await;
            if message.is_none() {
                shutdown.cancel();
            }
            message
        }
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.inner.close()
    }
}

const MCP_PROTOCOL: &str = "2025-11-25";
const MCP_SDK: &str = "rmcp/1.5.0";
const MAX_SOURCE_BYTES: usize = 65_536;
const MAX_TOOL_OUTPUT_BYTES: usize = 1_048_576;
const MAX_DIAGNOSTICS: usize = 256;
const CHECK_TIMEOUT_MILLIS: u64 = 5_000;

const PUBLIC_REFERENCE: &str = include_str!("../assets/topaz-5.20/reference.json");
const EXAMPLES: &str = include_str!("../assets/topaz-5.20/examples.json");

#[derive(Clone)]
struct TopazMcp {
    topaz: PathBuf,
    compiler: String,
    toolchain: JsonValue,
    execution: ExecutionRuntime,
    run_slots: Arc<Semaphore>,
    shutdown: CancellationToken,
}

#[derive(Deserialize)]
struct ExamplesAsset {
    schema: String,
    authority: String,
    examples: Vec<Example>,
}

#[derive(Deserialize)]
struct ReferenceAsset {
    schema: String,
    authority: String,
    pages: Vec<ReferencePage>,
}

#[derive(Deserialize)]
struct ReferencePage {
    route: String,
    slug: String,
    title: String,
    covers: String,
    markdown: String,
}

#[derive(Deserialize)]
struct Example {
    route: String,
    title: String,
    heading: String,
    source: String,
}

impl ServerHandler for TopazMcp {
    fn get_info(&self) -> ServerInfo {
        let version = self
            .toolchain
            .get("version")
            .and_then(JsonValue::as_str)
            .unwrap_or("unknown");
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(rmcp::model::ProtocolVersion::V_2025_11_25)
            .with_server_info(
                Implementation::new("topaz-mcp", env!("CARGO_PKG_VERSION"))
                    .with_title("Topaz Native stdio MCP"),
            )
            .with_instructions(format!(
                "Exact sections from the published Topaz documentation set, mechanically copied documentation examples, stateless installed-checker diagnostics, and built-in no-capability execution. Toolchain: {version}; compiler selection: {}. Execution: {}. No remote transport, submitted-source persistence, logging, cross-call state, component loading, or fallback.",
                self.compiler,
                self.execution.metadata()
            ))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: tool_definitions(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        match request.name.as_ref() {
            "topaz_spec" => {
                let arguments = closed_arguments(request.arguments, &["section"], "topaz_spec")?;
                let section = optional_string(&arguments, "section", 160)?;
                Ok(CallToolResult::structured(spec_result(section.as_deref())))
            }
            "topaz_examples" => {
                let arguments =
                    closed_arguments(request.arguments, &["concept"], "topaz_examples")?;
                let concept = required_string(&arguments, "concept", 160)?;
                examples_result(&concept)
                    .map(CallToolResult::structured)
                    .map_err(|message| ErrorData::internal_error(message, None))
            }
            "topaz_check" => {
                let arguments =
                    closed_arguments(request.arguments, &["source", "profile"], "topaz_check")?;
                let source = required_string(&arguments, "source", MAX_SOURCE_BYTES)?;
                let profile = check_profile_argument(&arguments)?;
                let topaz = self.topaz.clone();
                let compiler = self.compiler.clone();
                let compiler_for_worker = compiler.clone();
                let toolchain = self.toolchain.clone();
                let profile_for_worker = profile.clone();
                let result = tokio::task::spawn_blocking(move || {
                    check_source(
                        &topaz,
                        &compiler_for_worker,
                        &source,
                        profile_for_worker.as_deref(),
                    )
                })
                .await
                .map_err(|_| ErrorData::internal_error("Topaz checker worker failed", None))?;
                match result {
                    Ok(diagnostics) => {
                        let clean = diagnostics.is_empty();
                        Ok(CallToolResult::structured(check_result_profile(
                            json!({
                                "schema": "topaz.mcp-check-result/v1",
                                "status": if clean { "clean" } else { "diagnostics" },
                                "exit_status": if clean { 0 } else { 1 },
                                "diagnostics": diagnostics,
                                "compiler_selection": compiler,
                                "toolchain": toolchain,
                                "resource_profile": {
                                    "source_byte_limit": MAX_SOURCE_BYTES,
                                    "output_byte_limit": MAX_TOOL_OUTPUT_BYTES,
                                    "diagnostic_limit": MAX_DIAGNOSTICS,
                                    "wall_clock_millis": CHECK_TIMEOUT_MILLIS,
                                    "cpu_hard_bound_claimed": false,
                                    "operating_system_memory_ceiling": false
                                },
                                "source_retained": false,
                                "source_logged": false,
                                "state_between_calls": "none",
                                "fallback": false
                            }),
                            profile.as_deref(),
                        )))
                    }
                    Err(CheckFailure::Timeout) => {
                        Ok(CallToolResult::structured_error(check_result_profile(
                            json!({
                                "schema": "topaz.mcp-check-result/v1",
                                "status": "limit-exceeded",
                                "limit": "wall-clock",
                                "exit_status": 2,
                                "diagnostics": [],
                                "compiler_selection": compiler,
                                "toolchain": toolchain,
                                "source_retained": false,
                                "source_logged": false,
                                "state_between_calls": "none",
                                "fallback": false
                            }),
                            profile.as_deref(),
                        )))
                    }
                    Err(CheckFailure::Limit(limit)) => {
                        Ok(CallToolResult::structured_error(check_result_profile(
                            json!({
                                "schema": "topaz.mcp-check-result/v1",
                                "status": "limit-exceeded",
                                "limit": limit,
                                "exit_status": 2,
                                "diagnostics": [],
                                "compiler_selection": compiler,
                                "toolchain": toolchain,
                                "source_retained": false,
                                "source_logged": false,
                                "state_between_calls": "none",
                                "fallback": false
                            }),
                            profile.as_deref(),
                        )))
                    }
                    Err(CheckFailure::Infrastructure) => {
                        Ok(CallToolResult::structured_error(check_result_profile(
                            json!({
                                "schema": "topaz.mcp-check-result/v1",
                                "status": "infrastructure-error",
                                "exit_status": 2,
                                "diagnostics": [],
                                "compiler_selection": compiler,
                                "toolchain": toolchain,
                                "source_retained": false,
                                "source_logged": false,
                                "state_between_calls": "none",
                                "fallback": false
                            }),
                            profile.as_deref(),
                        )))
                    }
                }
            }
            "topaz_run" => {
                let arguments =
                    closed_arguments(request.arguments, &["source", "input"], "topaz_run")?;
                let source = required_string(&arguments, "source", MAX_SOURCE_BYTES)?;
                let input =
                    optional_string(&arguments, "input", MAX_SOURCE_BYTES)?.unwrap_or_default();
                let permit = match Arc::clone(&self.run_slots).try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        return Ok(CallToolResult::structured_error(self.execution.busy()));
                    }
                };
                let cancelled = Arc::new(AtomicBool::new(false));
                let request_cancelled = Arc::clone(&cancelled);
                let request_token = context.ct.clone();
                let request_watcher = tokio::spawn(async move {
                    request_token.cancelled().await;
                    request_cancelled.store(true, Ordering::Release);
                });
                let shutdown_cancelled = Arc::clone(&cancelled);
                let shutdown_token = self.shutdown.clone();
                let shutdown_watcher = tokio::spawn(async move {
                    shutdown_token.cancelled().await;
                    shutdown_cancelled.store(true, Ordering::Release);
                });
                let execution = self.execution.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    execution.run(source, input, cancelled)
                })
                .await
                .map_err(|_| ErrorData::internal_error("Topaz execution worker failed", None))?;
                request_watcher.abort();
                shutdown_watcher.abort();
                if result.get("status").and_then(JsonValue::as_str) == Some("completed") {
                    Ok(CallToolResult::structured(result))
                } else {
                    Ok(CallToolResult::structured_error(result))
                }
            }
            _ => Err(ErrorData::invalid_params(
                "unknown Topaz MCP tool",
                Some(json!({
                    "known_tools": ["topaz_spec", "topaz_examples", "topaz_check", "topaz_run"]
                })),
            )),
        }
    }
}

fn tool_definitions() -> Vec<Tool> {
    let annotations = || {
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false)
    };
    vec![
        Tool::new(
            "topaz_spec",
            "Return an exact section from the published Topaz documentation set. With no section, return the page and heading table with specification coverage. No summary is generated.",
            schema_object(json!({
                "type": "object",
                "properties": {
                    "section": { "type": "string", "maxLength": 160 }
                },
                "additionalProperties": false
            })),
        )
        .with_annotations(annotations()),
        Tool::new(
            "topaz_examples",
            "Return exact already-verified Topaz documentation samples whose published route, title, or heading matches one concept. No example is generated.",
            schema_object(json!({
                "type": "object",
                "properties": {
                    "concept": { "type": "string", "maxLength": 160 }
                },
                "required": ["concept"],
                "additionalProperties": false
            })),
        )
        .with_annotations(annotations()),
        Tool::new(
            "topaz_check",
            "Check bounded UTF-8 source through a fresh installed Topaz LSP process using the explicit compiler selection. Return the checker diagnostic objects without rewriting code, range, severity, source, or message.",
            schema_object(json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "maxLength": MAX_SOURCE_BYTES },
                    "profile": { "type": "string", "enum": ["agent-pack"] }
                },
                "required": ["source"],
                "additionalProperties": false
            })),
        )
        .with_annotations(annotations()),
        Tool::new(
            "topaz_run",
            "Check and execute bounded UTF-8 Topaz source in one fresh built-in no-capability worker with a five-second wall limit and a one-MiB response limit. This is not an operating-system sandbox or a hard CPU or RSS bound.",
            schema_object(json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "maxLength": MAX_SOURCE_BYTES },
                    "input": { "type": "string", "maxLength": MAX_SOURCE_BYTES }
                },
                "required": ["source"],
                "additionalProperties": false
            })),
        )
        .with_annotations(annotations()),
    ]
}

fn spec_result(section: Option<&str>) -> JsonValue {
    let asset: ReferenceAsset =
        serde_json::from_str(PUBLIC_REFERENCE).expect("packaged public reference must parse");
    assert_eq!(asset.schema, "topaz.mcp-public-reference/v1");
    let all_sections =
        asset
            .pages
            .iter()
            .flat_map(|page| {
                let route = page.route.clone();
                let covers = page.covers.clone();
                let title = page.title.clone();
                std::iter::once(json!({
                    "route": route,
                    "heading": title,
                    "covers": covers
                }))
                .chain(markdown_sections(&page.markdown).into_iter().map(
                    move |value| {
                        json!({
                            "route": page.route,
                            "heading": value.heading,
                            "covers": page.covers
                        })
                    },
                ))
            })
            .collect::<Vec<_>>();
    let Some(query) = section else {
        return json!({
            "schema": "topaz.mcp-authority-result/v1",
            "status": "index",
            "authority": asset.authority,
            "sections": all_sections
        });
    };
    let folded = query.to_lowercase();
    for page in &asset.pages {
        if page.title.to_lowercase() == folded
            || page.title.to_lowercase().contains(&folded)
            || page.slug.to_lowercase() == folded
        {
            return json!({
                "schema": "topaz.mcp-authority-result/v1",
                "status": "found",
                "authority": page.route,
                "heading": page.title,
                "covers": page.covers,
                "markdown": page.markdown
            });
        }
        for candidate in markdown_sections(&page.markdown) {
            if candidate.heading.to_lowercase() == folded
                || candidate.heading.to_lowercase().contains(&folded)
            {
                return json!({
                    "schema": "topaz.mcp-authority-result/v1",
                    "status": "found",
                    "authority": page.route,
                    "heading": candidate.heading,
                    "covers": page.covers,
                    "markdown": candidate.markdown
                });
            }
        }
    }
    json!({
        "schema": "topaz.mcp-authority-result/v1",
        "status": "absent",
        "query": query,
        "sections": all_sections
    })
}

struct MarkdownSection {
    heading: String,
    markdown: String,
}

fn markdown_sections(text: &str) -> Vec<MarkdownSection> {
    let lines = text.split_inclusive('\n').collect::<Vec<_>>();
    let starts = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            line.strip_prefix("## ")
                .map(|heading| (index, heading.trim().to_string()))
        })
        .collect::<Vec<_>>();
    starts
        .iter()
        .enumerate()
        .map(|(position, (start, heading))| {
            let end = starts
                .get(position + 1)
                .map(|(next, _)| *next)
                .unwrap_or(lines.len());
            MarkdownSection {
                heading: heading.clone(),
                markdown: lines[*start..end].concat(),
            }
        })
        .collect()
}

fn examples_result(concept: &str) -> Result<JsonValue, String> {
    let asset: ExamplesAsset = serde_json::from_str(EXAMPLES)
        .map_err(|_| "invalid packaged examples asset".to_string())?;
    let query = concept.to_lowercase();
    let examples = asset
        .examples
        .into_iter()
        .filter(|example| {
            format!("{} {} {}", example.route, example.title, example.heading)
                .to_lowercase()
                .contains(&query)
        })
        .take(24)
        .map(|example| {
            json!({
                "route": example.route,
                "title": example.title,
                "heading": example.heading,
                "source": example.source
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "schema": "topaz.mcp-examples-result/v1",
        "status": if examples.is_empty() { "absent" } else { "found" },
        "query": concept,
        "asset_schema": asset.schema,
        "authority": asset.authority,
        "examples": examples
    }))
}

fn check_result_profile(mut result: JsonValue, profile: Option<&str>) -> JsonValue {
    if let Some(profile) = profile {
        let object = result
            .as_object_mut()
            .expect("Topaz MCP check results are JSON objects");
        object.insert(
            "schema".to_string(),
            JsonValue::String("topaz.mcp-check-result/v2".to_string()),
        );
        object.insert(
            "profile".to_string(),
            JsonValue::String(profile.to_string()),
        );
    }
    result
}

#[derive(Debug)]
enum CheckFailure {
    Timeout,
    Limit(&'static str),
    Infrastructure,
}

fn check_source(
    topaz: &Path,
    compiler: &str,
    source: &str,
    profile: Option<&str>,
) -> Result<Vec<JsonValue>, CheckFailure> {
    let mut command = Command::new(topaz);
    command
        .args(["lsp", "--compiler", compiler])
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|_| CheckFailure::Infrastructure)?;
    let stdout = child.stdout.take().ok_or(CheckFailure::Infrastructure)?;
    let stderr = child.stderr.take().ok_or(CheckFailure::Infrastructure)?;
    let stdout_reader = thread::spawn(move || read_bounded(stdout, MAX_TOOL_OUTPUT_BYTES));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, 65_536));

    let uri = "file:///__topaz_mcp__.tpz";
    let initialize = match profile {
        Some(profile) => {
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{},"initializationOptions":{"topaz":{"checkProfile":profile}}}})
        }
        None => {
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{}}})
        }
    };
    let messages = [
        initialize,
        json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"topaz","version":1,"text":source}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}),
        json!({"jsonrpc":"2.0","method":"exit","params":null}),
    ];
    {
        let mut stdin = child.stdin.take().ok_or(CheckFailure::Infrastructure)?;
        for message in messages {
            let encoded = serde_json::to_vec(&message).map_err(|_| CheckFailure::Infrastructure)?;
            write!(stdin, "Content-Length: {}\r\n\r\n", encoded.len())
                .and_then(|_| stdin.write_all(&encoded))
                .map_err(|_| CheckFailure::Infrastructure)?;
        }
    }

    let deadline = Instant::now() + Duration::from_millis(CHECK_TIMEOUT_MILLIS);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|_| CheckFailure::Infrastructure)? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(CheckFailure::Timeout);
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| CheckFailure::Infrastructure)??;
    let _stderr = stderr_reader
        .join()
        .map_err(|_| CheckFailure::Infrastructure)??;
    if !status.success() {
        return Err(CheckFailure::Infrastructure);
    }
    let messages = parse_lsp_messages(&stdout)?;
    let diagnostics = messages
        .iter()
        .rev()
        .find(|message| {
            message.get("method").and_then(JsonValue::as_str)
                == Some("textDocument/publishDiagnostics")
                && message.pointer("/params/uri").and_then(JsonValue::as_str) == Some(uri)
        })
        .and_then(|message| message.pointer("/params/diagnostics"))
        .and_then(JsonValue::as_array)
        .cloned()
        .ok_or(CheckFailure::Infrastructure)?;
    if diagnostics.len() > MAX_DIAGNOSTICS {
        return Err(CheckFailure::Limit("diagnostic-count"));
    }
    Ok(diagnostics)
}

fn read_bounded<R: Read>(reader: R, limit: usize) -> Result<Vec<u8>, CheckFailure> {
    let mut bytes = Vec::new();
    reader
        .take(u64::try_from(limit + 1).map_err(|_| CheckFailure::Infrastructure)?)
        .read_to_end(&mut bytes)
        .map_err(|_| CheckFailure::Infrastructure)?;
    if bytes.len() > limit {
        return Err(CheckFailure::Limit("subprocess-output"));
    }
    Ok(bytes)
}

fn parse_lsp_messages(bytes: &[u8]) -> Result<Vec<JsonValue>, CheckFailure> {
    let mut cursor = 0;
    let mut messages = Vec::new();
    while cursor < bytes.len() {
        let header_end = bytes[cursor..]
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|position| cursor + position)
            .ok_or(CheckFailure::Infrastructure)?;
        let header = std::str::from_utf8(&bytes[cursor..header_end])
            .map_err(|_| CheckFailure::Infrastructure)?;
        let length = header
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .ok_or(CheckFailure::Infrastructure)?
            .parse::<usize>()
            .map_err(|_| CheckFailure::Infrastructure)?;
        let body_start = header_end + 4;
        let body_end = body_start
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or(CheckFailure::Infrastructure)?;
        messages.push(
            serde_json::from_slice(&bytes[body_start..body_end])
                .map_err(|_| CheckFailure::Infrastructure)?,
        );
        cursor = body_end;
    }
    Ok(messages)
}

fn toolchain_metadata(topaz: &Path) -> Result<JsonValue, String> {
    let version = Command::new(topaz)
        .args(["version", "--verbose"])
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| format!("cannot inspect Topaz version: {error}"))?;
    if !version.status.success() {
        return Err("Topaz version command failed".to_string());
    }
    let support = Command::new(topaz)
        .args(["compiler", "status", "--json"])
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| format!("cannot inspect Topaz compiler support: {error}"))?;
    if !support.status.success() {
        return Err("Topaz compiler status command failed".to_string());
    }
    let support: JsonValue =
        serde_json::from_slice(&support.stdout).map_err(|_| "invalid compiler status JSON")?;
    let mut file = File::open(topaz).map_err(|error| format!("cannot hash Topaz: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 65_536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot hash Topaz: {error}"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(json!({
        "distribution": "installed-native-topaz",
        "sha256": format!("{:x}", hasher.finalize()),
        "version": String::from_utf8_lossy(&version.stdout).trim(),
        "compiler_support": support,
        "mcp_protocol": MCP_PROTOCOL,
        "mcp_sdk": MCP_SDK
    }))
}

fn closed_arguments(
    arguments: Option<JsonObject<String, JsonValue>>,
    admitted: &[&str],
    tool: &str,
) -> Result<JsonObject<String, JsonValue>, ErrorData> {
    let arguments = arguments.unwrap_or_default();
    if let Some(name) = arguments
        .keys()
        .find(|name| !admitted.contains(&name.as_str()))
    {
        return Err(ErrorData::invalid_params(
            format!("{tool} does not accept argument {name}"),
            None,
        ));
    }
    Ok(arguments)
}

fn optional_string(
    arguments: &JsonObject<String, JsonValue>,
    name: &str,
    max_bytes: usize,
) -> Result<Option<String>, ErrorData> {
    let Some(value) = arguments.get(name) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .ok_or_else(|| ErrorData::invalid_params(format!("{name} must be a UTF-8 string"), None))?;
    if value.len() > max_bytes {
        return Err(ErrorData::invalid_params(
            format!("{name} exceeds the {max_bytes}-byte limit"),
            None,
        ));
    }
    Ok(Some(value.to_string()))
}

fn check_profile_argument(
    arguments: &JsonObject<String, JsonValue>,
) -> Result<Option<String>, ErrorData> {
    let profile = optional_string(arguments, "profile", 32)?;
    match profile.as_deref() {
        None | Some("agent-pack") => Ok(profile),
        Some("bootstrap") => Err(ErrorData::invalid_params(
            "profile `bootstrap` applies to a locked package; topaz_check checks one standalone source",
            None,
        )),
        Some(profile) => Err(ErrorData::invalid_params(
            format!("profile must be `agent-pack`; received `{profile}`"),
            None,
        )),
    }
}

fn required_string(
    arguments: &JsonObject<String, JsonValue>,
    name: &str,
    max_bytes: usize,
) -> Result<String, ErrorData> {
    optional_string(arguments, name, max_bytes)?
        .ok_or_else(|| ErrorData::invalid_params(format!("{name} is required"), None))
}

fn schema_object(value: JsonValue) -> Arc<JsonObject<String, JsonValue>> {
    Arc::new(
        value
            .as_object()
            .cloned()
            .expect("tool schema must be an object"),
    )
}

pub fn run_stdio_server(topaz: PathBuf, compiler: String) -> Result<(), String> {
    if !topaz.is_absolute() || !topaz.is_file() {
        return Err("installed Topaz executable is unavailable".to_string());
    }
    if !matches!(compiler.as_str(), "self" | "rust") {
        return Err("compiler must be self or rust".to_string());
    }
    let toolchain = toolchain_metadata(&topaz)?;
    let execution = ExecutionRuntime::load(topaz.clone(), env!("CARGO_PKG_VERSION").into())?;
    let run_slots = Arc::new(Semaphore::new(execution.maximum_active_runs()));
    let shutdown = CancellationToken::new();
    let shutdown_after_service = shutdown.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|error| format!("cannot create MCP runtime: {error}"))?;
    runtime.block_on(async {
        let transport = ShutdownAwareTransport {
            inner: stdio().into_transport(),
            shutdown: shutdown.clone(),
        };
        let service = TopazMcp {
            topaz,
            compiler,
            toolchain,
            execution,
            run_slots,
            shutdown,
        }
        .serve(transport)
        .await
        .map_err(|error| format!("cannot start MCP stdio service: {error}"))?;
        let result = service
            .waiting()
            .await
            .map(|_| ())
            .map_err(|error| format!("MCP stdio service failed: {error}"));
        shutdown_after_service.cancel();
        result
    })
}

pub fn run_worker_stdio() -> Result<(), String> {
    let request =
        topaz_mcp_worker::protocol::WorkerRequest::read_from(&mut std::io::stdin().lock())
            .map_err(|error| error.to_string())?;
    let response = topaz_mcp_worker::execute(request);
    let mut frame = Vec::new();
    response
        .write_to(&mut frame)
        .map_err(|error| error.to_string())?;
    std::io::stdout()
        .lock()
        .write_all(&frame)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unprofiled_check_result_keeps_the_v1_bytes() {
        let result = check_result_profile(
            json!({
                "schema": "topaz.mcp-check-result/v1",
                "status": "clean",
                "diagnostics": []
            }),
            None,
        );
        assert_eq!(
            serde_json::to_string(&result).expect("check result JSON"),
            r#"{"diagnostics":[],"schema":"topaz.mcp-check-result/v1","status":"clean"}"#
        );
    }

    #[test]
    fn profiled_check_results_identify_the_v2_profile_on_every_outcome() {
        for status in [
            "clean",
            "diagnostics",
            "limit-exceeded",
            "infrastructure-error",
        ] {
            let result = check_result_profile(
                json!({
                    "schema": "topaz.mcp-check-result/v1",
                    "status": status
                }),
                Some("agent-pack"),
            );
            assert_eq!(
                result.get("schema").and_then(JsonValue::as_str),
                Some("topaz.mcp-check-result/v2")
            );
            assert_eq!(
                result.get("profile").and_then(JsonValue::as_str),
                Some("agent-pack")
            );
        }
    }

    #[test]
    fn check_profile_validation_rejects_package_and_unknown_profiles() {
        for (value, expected) in [
            (json!("bootstrap"), "applies to a locked package"),
            (json!("nonsense"), "must be `agent-pack`"),
            (json!(42), "must be a UTF-8 string"),
        ] {
            let arguments = JsonObject::from_iter([("profile".to_string(), value)]);
            let error = check_profile_argument(&arguments).expect_err("invalid check profile");
            assert!(error.message.contains(expected), "{error:?}");
        }
    }
}
