//! Bounded HTTP/1 transport host for generated Topaz service artifacts.
//!
//! This crate owns transport, connection, and host configuration only. It does
//! not depend on `topaz_value`: generated service harnesses keep all Topaz
//! values on the current-thread reactor and exchange only owned
//! request/response records with the HTTP transport boundary.

use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{HeaderName, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Host-owned request data. Repeated headers retain their wire order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedRequest {
    pub request_id: u64,
    pub method: String,
    pub target: String,
    pub authority: String,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}

pub type HandlerFuture =
    Pin<Box<dyn Future<Output = Result<OwnedResponse, HandlerFault>> + 'static>>;

pub trait Handler: 'static {
    fn handle(&self, request: OwnedRequest) -> HandlerFuture;
}

impl<F, Fut> Handler for F
where
    F: Fn(OwnedRequest) -> Fut + 'static,
    Fut: Future<Output = Result<OwnedResponse, HandlerFault>> + 'static,
{
    fn handle(&self, request: OwnedRequest) -> HandlerFuture {
        Box::pin(self(request))
    }
}

/// Validated host response data. The service adapter constructs this only
/// after checking the Topaz `HttpResponse` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedResponse {
    pub status: u16,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerFaultKind {
    BadRequest,
    Timeout,
    Runtime,
    InvalidResponse,
    Overloaded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerFault {
    pub kind: HandlerFaultKind,
    pub diagnostic: String,
}

impl HandlerFault {
    pub fn new(kind: HandlerFaultKind, diagnostic: impl Into<String>) -> Self {
        Self {
            kind,
            diagnostic: diagnostic.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
    Off,
}

pub const CONFIG_SCHEMA: &str = "topaz.httpServiceConfig.v1";
pub const LOG_SCHEMA: &str = "topaz.httpServiceLog.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostConfig {
    pub bind: IpAddr,
    pub port: u16,
    pub workers: u16,
    pub max_connections: u16,
    pub queue_capacity: u16,
    pub max_target_bytes: usize,
    pub max_header_bytes: usize,
    pub max_headers: usize,
    pub max_body_bytes: usize,
    pub header_timeout: Duration,
    pub body_timeout: Duration,
    pub handler_timeout: Duration,
    pub shutdown_grace: Duration,
    pub log_format: LogFormat,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            bind: IpAddr::from([127, 0, 0, 1]),
            port: 8080,
            workers: 1,
            max_connections: 64,
            queue_capacity: 32,
            max_target_bytes: 8_192,
            max_header_bytes: 16_384,
            max_headers: 64,
            max_body_bytes: 1_048_576,
            header_timeout: Duration::from_millis(5_000),
            body_timeout: Duration::from_millis(5_000),
            handler_timeout: Duration::from_millis(1_000),
            shutdown_grace: Duration::from_millis(5_000),
            log_format: LogFormat::Text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(String);

impl ConfigError {
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

impl HostConfig {
    /// Revalidate embedded defaults and command-line overrides at process
    /// startup. Package parsing performs the same range checks earlier, but the
    /// generated artifact remains fail-closed independently of its source.
    pub fn validate(&self) -> Result<(), ConfigError> {
        bounded("port", u64::from(self.port), 1, u64::from(u16::MAX))?;
        bounded("workers", u64::from(self.workers), 1, 64)?;
        bounded("max-connections", u64::from(self.max_connections), 1, 4_096)?;
        bounded("queue-capacity", u64::from(self.queue_capacity), 0, 4_096)?;
        bounded(
            "max-target-bytes",
            self.max_target_bytes as u64,
            256,
            16_384,
        )?;
        bounded(
            "max-header-bytes",
            self.max_header_bytes as u64,
            1_024,
            65_536,
        )?;
        bounded("max-headers", self.max_headers as u64, 1, 128)?;
        bounded("max-body-bytes", self.max_body_bytes as u64, 0, 16_777_216)?;
        bounded_duration("header-timeout-ms", self.header_timeout, 100, 60_000)?;
        bounded_duration("body-timeout-ms", self.body_timeout, 100, 60_000)?;
        bounded_duration("handler-timeout-ms", self.handler_timeout, 10, 60_000)?;
        bounded_duration("shutdown-grace-ms", self.shutdown_grace, 0, 60_000)?;
        Ok(())
    }

    pub fn with_args<I>(mut self, args: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut args = args.into_iter().peekable();
        let mut seen = std::collections::BTreeSet::new();
        while let Some(argument) = args.next() {
            let (flag, inline) = match argument.split_once('=') {
                Some((flag, value)) => (flag.to_string(), Some(value.to_string())),
                None => (argument, None),
            };
            if !flag.starts_with("--") {
                return Err(ConfigError(format!("unexpected argument `{flag}`")));
            }
            if !seen.insert(flag.clone()) {
                return Err(ConfigError(format!("duplicate service option `{flag}`")));
            }
            let value = match inline {
                Some(value) if !value.is_empty() => value,
                Some(_) => return Err(ConfigError(format!("{flag} requires a value"))),
                None => args
                    .next()
                    .ok_or_else(|| ConfigError(format!("{flag} requires a value")))?,
            };
            match flag.as_str() {
                "--bind" => {
                    self.bind = value.parse().map_err(|_| {
                        ConfigError("--bind must be an IPv4 or IPv6 literal".to_string())
                    })?;
                }
                "--port" => self.port = parse_number(&flag, &value)?,
                "--workers" => self.workers = parse_number(&flag, &value)?,
                "--max-connections" => self.max_connections = parse_number(&flag, &value)?,
                "--queue-capacity" => self.queue_capacity = parse_number(&flag, &value)?,
                "--max-target-bytes" => self.max_target_bytes = parse_number(&flag, &value)?,
                "--max-header-bytes" => self.max_header_bytes = parse_number(&flag, &value)?,
                "--max-headers" => self.max_headers = parse_number(&flag, &value)?,
                "--max-body-bytes" => self.max_body_bytes = parse_number(&flag, &value)?,
                "--header-timeout-ms" => {
                    self.header_timeout = Duration::from_millis(parse_number(&flag, &value)?)
                }
                "--body-timeout-ms" => {
                    self.body_timeout = Duration::from_millis(parse_number(&flag, &value)?)
                }
                "--handler-timeout-ms" => {
                    self.handler_timeout = Duration::from_millis(parse_number(&flag, &value)?)
                }
                "--shutdown-grace-ms" => {
                    self.shutdown_grace = Duration::from_millis(parse_number(&flag, &value)?)
                }
                "--log-format" => {
                    self.log_format = match value.as_str() {
                        "text" => LogFormat::Text,
                        "json" => LogFormat::Json,
                        "off" => LogFormat::Off,
                        _ => {
                            return Err(ConfigError(
                                "--log-format must be `text`, `json`, or `off`".to_string(),
                            ));
                        }
                    }
                }
                _ => return Err(ConfigError(format!("unknown service option `{flag}`"))),
            }
        }
        self.validate()?;
        Ok(self)
    }

    /// Render the validated process configuration without opening a listener.
    /// Generated services expose this through `--print-config` so supervisors
    /// can inspect the exact effective bounds after command-line overrides.
    pub fn effective_json(&self) -> Result<String, ConfigError> {
        self.validate()?;
        let log_format = match self.log_format {
            LogFormat::Text => "text",
            LogFormat::Json => "json",
            LogFormat::Off => "off",
        };
        Ok(format!(
            concat!(
                "{{\n",
                "  \"schema\": \"{}\",\n",
                "  \"source\": \"effective-runtime\",\n",
                "  \"transport\": \"http1\",\n",
                "  \"runtimeOverrides\": \"command-line\",\n",
                "  \"nonLoopbackBind\": \"explicit-only\",\n",
                "  \"values\": {{\n",
                "    \"bind\": \"{}\",\n",
                "    \"port\": {},\n",
                "    \"workers\": {},\n",
                "    \"maxConnections\": {},\n",
                "    \"queueCapacity\": {},\n",
                "    \"maxTargetBytes\": {},\n",
                "    \"maxHeaderBytes\": {},\n",
                "    \"maxHeaders\": {},\n",
                "    \"maxBodyBytes\": {},\n",
                "    \"headerTimeoutMs\": {},\n",
                "    \"bodyTimeoutMs\": {},\n",
                "    \"handlerTimeoutMs\": {},\n",
                "    \"shutdownGraceMs\": {},\n",
                "    \"logFormat\": \"{}\"\n",
                "  }}\n",
                "}}\n"
            ),
            CONFIG_SCHEMA,
            self.bind,
            self.port,
            self.workers,
            self.max_connections,
            self.queue_capacity,
            self.max_target_bytes,
            self.max_header_bytes,
            self.max_headers,
            self.max_body_bytes,
            self.header_timeout.as_millis(),
            self.body_timeout.as_millis(),
            self.handler_timeout.as_millis(),
            self.shutdown_grace.as_millis(),
            log_format,
        ))
    }
}

fn parse_number<T>(flag: &str, value: &str) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| ConfigError(format!("{flag} requires a decimal integer (got `{value}`)")))
}

#[derive(Debug)]
pub enum ServeError {
    Config(ConfigError),
    Runtime(std::io::Error),
    Bind(std::io::Error),
    Signal(std::io::Error),
}

impl fmt::Display for ServeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(f, "invalid service configuration: {error}"),
            Self::Runtime(error) => write!(f, "cannot start service runtime: {error}"),
            Self::Bind(error) => write!(f, "cannot bind service listener: {error}"),
            Self::Signal(error) => write!(f, "cannot install shutdown signal handler: {error}"),
        }
    }
}

impl std::error::Error for ServeError {}

/// Run the bounded HTTP/1 host on a current-thread Tokio reactor. Generated
/// handlers are local futures, so non-`Send` Topaz values never cross a thread
/// boundary; the worker semaphore bounds active request evaluations.
pub fn serve(config: HostConfig, handler: Rc<dyn Handler>) -> Result<(), ServeError> {
    config.validate().map_err(ServeError::Config)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(ServeError::Runtime)?;
    let local = tokio::task::LocalSet::new();
    runtime.block_on(local.run_until(serve_async(config, handler)))
}

async fn serve_async(config: HostConfig, handler: Rc<dyn Handler>) -> Result<(), ServeError> {
    let listener = TcpListener::bind((config.bind, config.port))
        .await
        .map_err(ServeError::Bind)?;
    let address = listener.local_addr().map_err(ServeError::Bind)?;
    log_host(
        &config,
        0,
        "service-started",
        &format!("address=http://{address}"),
    );
    let permits = Arc::new(Semaphore::new(usize::from(config.max_connections)));
    let handler_capacity = Arc::new(Semaphore::new(
        usize::from(config.workers) + usize::from(config.queue_capacity),
    ));
    let workers = Arc::new(Semaphore::new(usize::from(config.workers)));
    let request_ids = Arc::new(AtomicU64::new(1));
    let config = Arc::new(config);
    let mut connections = JoinSet::new();
    let mut shutdown = Box::pin(shutdown_signal());

    loop {
        while connections.try_join_next().is_some() {}
        let mut accept = std::pin::pin!(listener.accept());
        let event = std::future::poll_fn(|context| {
            if let std::task::Poll::Ready(signal) = shutdown.as_mut().poll(context) {
                return std::task::Poll::Ready(Either::Shutdown(signal));
            }
            if let std::task::Poll::Ready(accepted) = accept.as_mut().poll(context) {
                return std::task::Poll::Ready(Either::Accepted(accepted));
            }
            std::task::Poll::Pending
        })
        .await;
        match event {
            Either::Shutdown(signal) => {
                if let Err(error) = signal {
                    log_host(&config, 0, "shutdown-signal-error", &error.to_string());
                    return Err(ServeError::Signal(error));
                }
                log_host(&config, 0, "shutdown-requested", "signal received");
                break;
            }
            Either::Accepted(accepted) => {
                let (stream, _) = match accepted {
                    Ok(pair) => pair,
                    Err(error) => {
                        log_host(&config, 0, "accept-error", &error.to_string());
                        continue;
                    }
                };
                let Ok(permit) = permits.clone().try_acquire_owned() else {
                    log_host(
                        &config,
                        0,
                        "connection-overload",
                        "connection limit reached",
                    );
                    let _ = stream.try_write(
                        b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Type: text/plain; charset=utf-8\r\nX-Topaz-Request-Id: 0\r\nContent-Length: 18\r\n\r\nservice overloaded",
                    );
                    drop(stream);
                    continue;
                };
                let handler = handler.clone();
                let connection_config = config.clone();
                let ids = request_ids.clone();
                let request_capacity = handler_capacity.clone();
                let request_workers = workers.clone();
                connections.spawn_local(async move {
                    let io = TokioIo::new(stream);
                    let service_config = connection_config.clone();
                    let service = service_fn(move |request| {
                        handle_http_request(
                            request,
                            handler.clone(),
                            service_config.clone(),
                            ids.clone(),
                            request_capacity.clone(),
                            request_workers.clone(),
                        )
                    });
                    let mut builder = http1::Builder::new();
                    builder
                        .timer(TokioTimer::new())
                        .header_read_timeout(connection_config.header_timeout)
                        .max_headers(connection_config.max_headers)
                        .max_buf_size(connection_config.max_header_bytes.max(8_192));
                    if let Err(error) = builder.serve_connection(io, service).await {
                        log_host(
                            &connection_config,
                            0,
                            "connection-error",
                            &error.to_string(),
                        );
                    }
                    drop(permit);
                });
            }
        }
    }

    drop(listener);
    let drained = tokio::time::timeout(config.shutdown_grace, async {
        while connections.join_next().await.is_some() {}
    })
    .await;
    match drained {
        Ok(()) => log_host(&config, 0, "shutdown-complete", "connections drained"),
        Err(_) => {
            connections.abort_all();
            while connections.join_next().await.is_some() {}
            log_host(&config, 0, "shutdown-forced", "shutdown grace elapsed");
        }
    }
    Ok(())
}

#[cfg(unix)]
async fn shutdown_signal() -> std::io::Result<()> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = Box::pin(tokio::signal::ctrl_c());
    let mut termination = Box::pin(terminate.recv());
    std::future::poll_fn(|context| {
        if let std::task::Poll::Ready(result) = interrupt.as_mut().poll(context) {
            return std::task::Poll::Ready(result);
        }
        if let std::task::Poll::Ready(signal) = termination.as_mut().poll(context) {
            return std::task::Poll::Ready(match signal {
                Some(()) => Ok(()),
                None => Err(std::io::Error::other("SIGTERM listener closed")),
            });
        }
        std::task::Poll::Pending
    })
    .await
}

#[cfg(not(unix))]
async fn shutdown_signal() -> std::io::Result<()> {
    tokio::signal::ctrl_c().await
}

enum Either<L, R> {
    Shutdown(L),
    Accepted(R),
}

async fn handle_http_request(
    request: Request<Incoming>,
    handler: Rc<dyn Handler>,
    config: Arc<HostConfig>,
    request_ids: Arc<AtomicU64>,
    handler_capacity: Arc<Semaphore>,
    workers: Arc<Semaphore>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let request_id = request_ids.fetch_add(1, Ordering::Relaxed);
    let (parts, body) = request.into_parts();
    let target = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/")
        .to_string();
    if target.len() > config.max_target_bytes {
        return Ok(request_response(
            &config,
            request_id,
            StatusCode::URI_TOO_LONG,
            "request target too large",
        ));
    }
    if parts.headers.len() > config.max_headers {
        return Ok(request_response(
            &config,
            request_id,
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "too many request headers",
        ));
    }
    let mut header_bytes = 0usize;
    let mut headers = Vec::with_capacity(parts.headers.len());
    for (name, value) in &parts.headers {
        header_bytes = header_bytes
            .saturating_add(name.as_str().len())
            .saturating_add(value.as_bytes().len());
        headers.push((name.as_str().to_string(), value.as_bytes().to_vec()));
    }
    if header_bytes > config.max_header_bytes {
        return Ok(request_response(
            &config,
            request_id,
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "request headers too large",
        ));
    }
    let authority = match parts.headers.get(hyper::header::HOST) {
        Some(value) => match value.to_str() {
            Ok(value) if !value.is_empty() => value.to_string(),
            _ => {
                return Ok(request_response(
                    &config,
                    request_id,
                    StatusCode::BAD_REQUEST,
                    "invalid Host header",
                ));
            }
        },
        None => {
            return Ok(request_response(
                &config,
                request_id,
                StatusCode::BAD_REQUEST,
                "missing Host header",
            ));
        }
    };
    if parts
        .headers
        .get(hyper::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > config.max_body_bytes)
    {
        return Ok(request_response(
            &config,
            request_id,
            StatusCode::PAYLOAD_TOO_LARGE,
            "request body too large",
        ));
    }
    let collected = tokio::time::timeout(
        config.body_timeout,
        Limited::new(body, config.max_body_bytes).collect(),
    )
    .await;
    let body = match collected {
        Err(_) => {
            return Ok(request_response(
                &config,
                request_id,
                StatusCode::REQUEST_TIMEOUT,
                "request body timeout",
            ));
        }
        Ok(Err(_)) => {
            return Ok(request_response(
                &config,
                request_id,
                StatusCode::PAYLOAD_TOO_LARGE,
                "request body too large",
            ));
        }
        Ok(Ok(body)) => body.to_bytes().to_vec(),
    };
    let request = OwnedRequest {
        request_id,
        method: parts.method.as_str().to_string(),
        target,
        authority,
        headers,
        body,
    };
    let Ok(capacity_permit) = handler_capacity.try_acquire_owned() else {
        return Ok(request_response(
            &config,
            request_id,
            StatusCode::SERVICE_UNAVAILABLE,
            "service overloaded",
        ));
    };
    let worker_permit = match workers.acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => {
            return Ok(request_response(
                &config,
                request_id,
                StatusCode::SERVICE_UNAVAILABLE,
                "service unavailable",
            ));
        }
    };
    let handled = tokio::time::timeout(
        config.handler_timeout,
        drive_handler(handler.handle(request)),
    )
    .await;
    drop(worker_permit);
    drop(capacity_permit);
    let response = match handled {
        Err(_) => {
            log_host(&config, request_id, "handler-timeout", "deadline exceeded");
            host_response(StatusCode::GATEWAY_TIMEOUT, "Gateway Timeout")
        }
        Ok(Ok(response)) => match checked_response(response, config.max_body_bytes) {
            Ok(response) => response,
            Err(message) => {
                log_host(&config, request_id, "invalid-response", &message);
                host_response(StatusCode::INTERNAL_SERVER_ERROR, "internal service error")
            }
        },
        Ok(Err(fault)) => {
            let (status, code) = match fault.kind {
                HandlerFaultKind::BadRequest => (StatusCode::BAD_REQUEST, "request-adapter"),
                HandlerFaultKind::Timeout => (StatusCode::GATEWAY_TIMEOUT, "handler-timeout"),
                HandlerFaultKind::Overloaded => {
                    (StatusCode::SERVICE_UNAVAILABLE, "handler-overload")
                }
                HandlerFaultKind::Runtime => (StatusCode::INTERNAL_SERVER_ERROR, "handler-fault"),
                HandlerFaultKind::InvalidResponse => {
                    (StatusCode::INTERNAL_SERVER_ERROR, "invalid-response")
                }
            };
            log_host(&config, request_id, code, &fault.diagnostic);
            host_response(status, status.canonical_reason().unwrap_or("service error"))
        }
    };
    log_host(
        &config,
        request_id,
        "request-complete",
        &format!("status={}", response.status().as_u16()),
    );
    Ok(with_request_id(response, request_id))
}

/// Poll one Topaz handler step at a time and yield to the reactor between
/// cooperative suspension points. A self-waking generated checkpoint must not
/// monopolize the current-thread `LocalSet` and starve accept/overload work.
async fn drive_handler(mut handler: HandlerFuture) -> Result<OwnedResponse, HandlerFault> {
    loop {
        let step = std::future::poll_fn(|context| {
            std::task::Poll::Ready(match handler.as_mut().poll(context) {
                std::task::Poll::Ready(output) => Some(output),
                std::task::Poll::Pending => None,
            })
        })
        .await;
        if let Some(output) = step {
            return output;
        }
        tokio::task::yield_now().await;
    }
}

fn checked_response(
    response: OwnedResponse,
    max_body_bytes: usize,
) -> Result<Response<Full<Bytes>>, String> {
    if response.body.len() > max_body_bytes {
        return Err("response body exceeds the configured maximum".to_string());
    }
    let status = StatusCode::from_u16(response.status).map_err(|_| "status outside 100..599")?;
    let mut output = Response::builder().status(status);
    let Some(headers) = output.headers_mut() else {
        return Err("response builder did not expose headers".to_string());
    };
    for (name, value) in response.headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| "response header name is invalid")?;
        if is_hop_by_hop(&name) || name == hyper::header::CONTENT_LENGTH {
            return Err(format!("response header `{name}` is host-owned"));
        }
        let value = HeaderValue::from_bytes(&value)
            .map_err(|_| format!("response header `{name}` has an invalid value"))?;
        headers.append(name, value);
    }
    output
        .body(Full::new(Bytes::from(response.body)))
        .map_err(|error| format!("cannot build response: {error}"))
}

fn is_hop_by_hop(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn host_response(status: StatusCode, message: &'static str) -> Response<Full<Bytes>> {
    let mut response = Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "text/plain; charset=utf-8");
    if status == StatusCode::SERVICE_UNAVAILABLE {
        response = response.header(hyper::header::CONNECTION, "close");
    }
    response
        .body(Full::new(Bytes::from_static(message.as_bytes())))
        .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())))
}

fn request_response(
    config: &HostConfig,
    request_id: u64,
    status: StatusCode,
    message: &'static str,
) -> Response<Full<Bytes>> {
    log_host(
        config,
        request_id,
        "request-rejected",
        &format!("status={}", status.as_u16()),
    );
    with_request_id(host_response(status, message), request_id)
}

fn with_request_id(mut response: Response<Full<Bytes>>, request_id: u64) -> Response<Full<Bytes>> {
    if let Ok(value) = HeaderValue::from_str(&request_id.to_string()) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-topaz-request-id"), value);
    }
    response
}

fn log_host(config: &HostConfig, request_id: u64, code: &str, diagnostic: &str) {
    match config.log_format {
        LogFormat::Off => {}
        LogFormat::Text => {
            eprintln!("topaz-service: request={request_id} code={code} diagnostic={diagnostic}")
        }
        LogFormat::Json => eprintln!(
            "{{\"schema\":\"{}\",\"requestId\":{request_id},\"code\":\"{}\",\"diagnostic\":\"{}\"}}",
            LOG_SCHEMA,
            json_escape(code),
            json_escape(diagnostic)
        ),
    }
}

fn json_escape(value: &str) -> String {
    let mut output = String::new();
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => output.push('?'),
            character => output.push(character),
        }
    }
    output
}

fn bounded(name: &str, value: u64, min: u64, max: u64) -> Result<(), ConfigError> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(ConfigError(format!(
            "--{name} must be in {min}..={max} (got {value})"
        )))
    }
}

fn bounded_duration(name: &str, value: Duration, min: u64, max: u64) -> Result<(), ConfigError> {
    let millis = u64::try_from(value.as_millis()).unwrap_or(u64::MAX);
    bounded(name, millis, min, max)
}

/// Pins the transport stack in compiled code as well as Cargo metadata. This
/// string is included in the generated service contract receipt.
pub const HOST_STACK_ID: &str =
    "hyper/1.11.0 tokio/1.53.1 hyper-util/0.1.20 http-body-util/0.1.4 http1-only";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_finite_and_valid() {
        HostConfig::default().validate().expect("valid defaults");
        assert!(HOST_STACK_ID.contains("http1-only"));
    }

    #[test]
    fn startup_validation_rejects_out_of_range_overrides() {
        let config = HostConfig {
            workers: 65,
            ..HostConfig::default()
        };
        assert_eq!(
            config.validate().expect_err("worker ceiling").message(),
            "--workers must be in 1..=64 (got 65)"
        );
    }

    #[test]
    fn startup_overrides_are_strict_and_duplicate_free() {
        let config = HostConfig::default()
            .with_args([
                "--port=9090".to_string(),
                "--workers".to_string(),
                "2".to_string(),
                "--log-format".to_string(),
                "json".to_string(),
            ])
            .expect("valid overrides");
        assert_eq!(config.port, 9090);
        assert_eq!(config.workers, 2);
        assert_eq!(config.log_format, LogFormat::Json);

        let duplicate = HostConfig::default()
            .with_args(["--port=9090".to_string(), "--port=9091".to_string()])
            .expect_err("duplicate rejected");
        assert!(duplicate.message().contains("duplicate service option"));
        let unknown = HostConfig::default()
            .with_args(["--mystery=1".to_string()])
            .expect_err("unknown rejected");
        assert!(unknown.message().contains("unknown service option"));
    }

    #[test]
    fn effective_configuration_is_versioned_validated_and_complete() {
        let config = HostConfig::default()
            .with_args([
                "--bind=0.0.0.0".to_string(),
                "--port=9090".to_string(),
                "--workers=2".to_string(),
                "--log-format=json".to_string(),
            ])
            .expect("valid effective configuration");
        let json = config.effective_json().expect("effective JSON");
        assert!(json.contains(&format!("\"schema\": \"{CONFIG_SCHEMA}\"")));
        assert!(json.contains("\"source\": \"effective-runtime\""));
        assert!(json.contains("\"bind\": \"0.0.0.0\""));
        assert!(json.contains("\"port\": 9090"));
        assert!(json.contains("\"workers\": 2"));
        assert!(json.contains("\"logFormat\": \"json\""));
        assert!(!json.contains("headerValues"));
        assert!(!json.contains("bodyValues"));
    }

    #[test]
    fn response_validation_rejects_host_owned_and_invalid_headers() {
        let response = OwnedResponse {
            status: 200,
            headers: vec![("content-length".to_string(), b"99".to_vec())],
            body: b"ok".to_vec(),
        };
        assert_eq!(
            checked_response(response, 16).expect_err("host-owned header"),
            "response header `content-length` is host-owned"
        );

        let response = OwnedResponse {
            status: 200,
            headers: vec![("x-test".to_string(), b"bad\nvalue".to_vec())],
            body: Vec::new(),
        };
        assert!(
            checked_response(response, 16)
                .expect_err("invalid value")
                .contains("invalid value")
        );

        let response = OwnedResponse {
            status: 200,
            headers: Vec::new(),
            body: b"too large".to_vec(),
        };
        assert_eq!(
            checked_response(response, 8).expect_err("oversized response"),
            "response body exceeds the configured maximum"
        );
    }
}
