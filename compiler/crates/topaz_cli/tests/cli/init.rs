use super::support::*;

#[test]
fn init_scaffolds_a_package_without_overwriting() {
    let dir = std::env::temp_dir().join("topaz_init_scaffold_test");
    let _ = std::fs::remove_dir_all(&dir);

    let out = topaz()
        .arg("init")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let manifest = std::fs::read_to_string(dir.join("topaz.toml")).expect("manifest");
    assert!(
        manifest.contains("name = \"topaz_init_scaffold_test\""),
        "{manifest}"
    );
    assert!(manifest.contains("language = \"5.20\""), "{manifest}");
    assert!(manifest.contains("std = \"5.20\""), "{manifest}");
    let entry = std::fs::read_to_string(dir.join("src/main.tpz")).expect("entry");
    assert!(
        entry.contains("export function main(args: Array<string>, stdin: string)"),
        "{entry}"
    );

    let out = topaz()
        .arg("run")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Hello from Topaz\n");

    let out = topaz()
        .arg("lock")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(dir.join("topaz.lock").exists(), "lockfile written");

    let out = topaz()
        .arg("init")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("refusing to overwrite"),
        "{out:?}"
    );

    let old_lang_dir = std::env::temp_dir().join("topaz_init_old_lang_test");
    let _ = std::fs::remove_dir_all(&old_lang_dir);
    let out = topaz()
        .arg("init")
        .arg("--language-version")
        .arg("5.3")
        .arg("--root")
        .arg(&old_lang_dir)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("scaffolds v5.20 packages"),
        "{out:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&old_lang_dir);
}

#[test]
fn init_web_app_scaffolds_checked_lifecycle_and_effective_default() {
    let dir = std::env::temp_dir().join("topaz_init_web_app_test");
    let out_dir = std::env::temp_dir().join("topaz_init_web_app_output");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&out_dir);

    let out = topaz()
        .args(["init", "--target", "web-app", "--root"])
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let manifest = std::fs::read_to_string(dir.join("topaz.toml")).expect("manifest");
    assert!(manifest.contains("target = \"web-app\""), "{manifest}");
    assert!(manifest.contains("[web]"), "{manifest}");
    assert!(manifest.contains("lifecycle = \"v2\""), "{manifest}");
    let source = std::fs::read_to_string(dir.join("src/main.tpz")).expect("source");
    assert!(source.contains("WebAppStep"), "{source}");
    assert!(source.contains("WebAppEvent"), "{source}");
    assert!(dir.join("styles/app.css").is_file());
    assert!(dir.join("tests/app.tpz").is_file());

    let checked = topaz()
        .args(["check", "--root"])
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(checked.status.success(), "{checked:?}");

    let installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .expect("rustup runs");
    if String::from_utf8_lossy(&installed.stdout).contains("wasm32-unknown-unknown") {
        let built = topaz()
            .args(["build", "--root"])
            .arg(&dir)
            .arg("--out-dir")
            .arg(&out_dir)
            .output()
            .expect("binary runs");
        assert!(built.status.success(), "{built:?}");
        for path in [
            "index.html",
            "topaz-app.js",
            "topaz-web.js",
            "topaz-web.d.ts",
            "topaz-web.wasm",
            "topaz-web-capabilities.json",
            "styles/app.css",
            "topaz-artifact.json",
            "LICENSE",
            "NOTICE",
            "GENERATED-OUTPUT-NOTICE.txt",
        ] {
            assert!(out_dir.join(path).is_file(), "missing {path}: {built:?}");
        }
        let artifact = std::fs::read_to_string(out_dir.join("topaz-artifact.json"))
            .expect("artifact manifest");
        assert!(artifact.contains("\"target\": \"web-app\""), "{artifact}");
        let host = std::fs::read_to_string(out_dir.join("topaz-app.js")).expect("host");
        assert!(host.contains("MAX_COMMANDS"), "{host}");
        assert!(host.contains("document.createTextNode"), "{host}");
        assert!(host.contains("\"option\""), "{host}");
        assert!(host.contains("\"selected\""), "{host}");
        for tag in [
            "\"blockquote\"",
            "\"del\"",
            "\"hr\"",
            "\"table\"",
            "\"thead\"",
            "\"tbody\"",
            "\"tr\"",
            "\"th\"",
            "\"td\"",
        ] {
            assert!(
                host.contains(tag),
                "missing safe structural tag {tag}: {host}"
            );
        }
        assert!(!host.contains("\"script\""), "{host}");
        assert!(
            host.contains("element instanceof HTMLTextAreaElement) element.value = value"),
            "{host}"
        );
        assert!(
            host.contains(&format!(
                "EXPECTED_TOOLCHAIN_VERSION = \"{}\"",
                env!("CARGO_PKG_VERSION")
            )),
            "{host}"
        );
        assert!(host.contains("host/runtime version mismatch"), "{host}");
        assert!(host.contains("const WEB_LIFECYCLE = \"v2\""), "{host}");
        assert!(
            host.contains("\"WebAppEvent.Browser\": \"0\"")
                && host.contains("\"LocalDataResult.Failed\": \"3\""),
            "{host}"
        );
        assert!(
            host.contains("OpenText requires [capabilities.web].open_text = true")
                && host.contains("DownloadText requires [capabilities.web].download_text = true"),
            "{host}"
        );
        assert!(host.contains("duplicate live local request id"), "{host}");
        assert!(
            host.contains("new TextDecoder(\"utf-8\", { fatal: true })"),
            "{host}"
        );
        assert!(host.contains("openText: false"), "{host}");
        assert!(host.contains("downloadText: false"), "{host}");
        assert!(host.contains("localState: false"), "{host}");
        assert!(
            host.contains("LoadState requires [capabilities.web].local_state = true")
                && host.contains("topaz.web-state.v1:topaz_init_web_app_test:")
                && host.contains("\"WebAppEvent.LocalState\": \"2\"")
                && host.contains("\"LocalStateResult.Failed\": \"3\"")
                && host.contains("const MAX_STATE_VALUE_BYTES = 1048576")
                && host.contains("const MAX_STATE_KEYS = 32")
                && host.contains("state-key-budget")
                && host.contains("state-corrupt")
                && host.contains("state-denied")
                && host.contains("state-quota")
                && host.contains("unknown or stale local-state completion"),
            "{host}"
        );
        assert!(!host.contains("__TOPAZ_"), "{host}");
        assert!(!host.contains("innerHTML"), "{host}");
        let capabilities = std::fs::read_to_string(out_dir.join("topaz-web-capabilities.json"))
            .expect("capabilities");
        assert!(
            capabilities.contains("\"schema\": \"topaz.web-capabilities.v1\""),
            "{capabilities}"
        );
        assert!(
            capabilities.contains("\"lifecycle\": \"v2\""),
            "{capabilities}"
        );
        assert!(
            capabilities.contains("\"localState\": false")
                && capabilities.contains(
                    "\"stateNamespace\": \"topaz.web-state.v1:topaz_init_web_app_test:\""
                )
                && capabilities.contains("\"maxStateValueBytes\": 1048576")
                && capabilities.contains("\"maxStateKeys\": 32"),
            "{capabilities}"
        );
        let loader = std::fs::read_to_string(out_dir.join("topaz-web.js")).expect("loader");
        assert!(
            loader.contains(&format!(
                "export const TOPAZ_TOOLCHAIN_VERSION = \"{}\"",
                env!("CARGO_PKG_VERSION")
            )),
            "{loader}"
        );
        let types = std::fs::read_to_string(out_dir.join("topaz-web.d.ts")).expect("types");
        assert!(
            types.contains(&format!(
                "export const TOPAZ_TOOLCHAIN_VERSION: \"{}\"",
                env!("CARGO_PKG_VERSION")
            )),
            "{types}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&out_dir);
}

#[test]
fn init_http_service_scaffolds_checked_handler_and_bounded_defaults() {
    let dir = std::env::temp_dir().join("topaz_init_http_service_test");
    let _ = std::fs::remove_dir_all(&dir);

    let out = topaz()
        .args(["init", "--target", "http-service", "--root"])
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let manifest = std::fs::read_to_string(dir.join("topaz.toml")).expect("manifest");
    assert!(manifest.contains("target = \"http-service\""), "{manifest}");
    assert!(manifest.contains("[service]"), "{manifest}");
    assert!(manifest.contains("bind = \"127.0.0.1\""), "{manifest}");
    assert!(manifest.contains("max_body_bytes = 1048576"), "{manifest}");
    assert!(manifest.contains("handler_timeout_ms = 1000"), "{manifest}");
    let source = std::fs::read_to_string(dir.join("src/main.tpz")).expect("source");
    assert!(source.contains("HttpRequest"), "{source}");
    assert!(source.contains("HttpResponse"), "{source}");
    assert!(source.contains("export function handle"), "{source}");

    let checked = topaz()
        .args(["check", "--root"])
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(checked.status.success(), "{checked:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn http_service_handler_shape_fails_before_host_scaffold() {
    let base = std::env::temp_dir().join("topaz_http_service_handler_shape_test");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("src")).expect("src");
    std::fs::write(
        base.join("topaz.toml"),
        "[package]\nname = \"bad_service_handler\"\nversion = \"0.1.0\"\nlanguage = \"5.9\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"http-service\"\n",
    )
    .expect("manifest");

    for (name, source, expected) in [
        (
            "missing",
            "import std.http { HttpRequest, HttpResponse }\nexport const other = 1\n",
            "missing required exported function `handle`",
        ),
        (
            "generic",
            "import std.http { HttpRequest, HttpResponse, text }\nexport function handle<T>(req: HttpRequest) -> HttpResponse { text(200, \"ok\") }\n",
            "`handle` must not be generic",
        ),
        (
            "wrong-request",
            "import std.http { HttpResponse, text }\nexport function handle(req: string) -> HttpResponse { text(200, req) }\n",
            "must have type `(std.http.HttpRequest) -> std.http.HttpResponse`",
        ),
        (
            "wrong-response",
            "import std.http { HttpRequest }\nexport function handle(req: HttpRequest) -> string { \"bad\" }\n",
            "must have type `(std.http.HttpRequest) -> std.http.HttpResponse`",
        ),
    ] {
        std::fs::write(base.join("src/main.tpz"), source).expect("entry");
        let out_dir = base.join(format!("out-{name}"));
        let out = topaz()
            .args(["build", "--root"])
            .arg(&base)
            .arg("--out-dir")
            .arg(&out_dir)
            .output()
            .expect("binary runs");
        assert!(!out.status.success(), "{name}: {out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("invalid http-service handler"),
            "{name}: {stderr}"
        );
        assert!(stderr.contains(expected), "{name}: {stderr}");
    }

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn http_service_build_runs_loopback_and_recovers_after_timeout() {
    let base = std::env::temp_dir().join("topaz_http_service_loopback_test");
    let output = base.join("product");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("src")).expect("src");
    std::fs::write(
        base.join("topaz.toml"),
        "[package]\nname = \"service_loopback\"\nversion = \"0.1.0\"\nlanguage = \"5.9\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"http-service\"\n\n[service]\nworkers = 1\nqueue_capacity = 0\nmax_connections = 2\nmax_body_bytes = 16\nheader_timeout_ms = 100\nbody_timeout_ms = 100\nhandler_timeout_ms = 500\nlog_format = \"json\"\n",
    )
    .expect("manifest");
    std::fs::write(
        base.join("src/main.tpz"),
        "import std.http { HttpRequest, HttpResponse, response, text }\n\nlet mut requestCount = 0\n\nexport function handle(req: HttpRequest) -> HttpResponse {\n  requestCount += 1\n  if req.url.path() == \"/spin\" {\n    while true {\n      let value = 1\n    }\n  }\n  if req.url.path() == \"/invalid\" {\n    return response(99, map {}, Bytes.encodeUtf8(\"private invalid response\"))\n  }\n  if req.url.path() == \"/large\" {\n    return text(200, \"private-response-17\")\n  }\n  if req.url.path() == \"/fault\" {\n    let broken = 1 / 0\n    return text(200, \"{broken}\")\n  }\n  text(200, \"healthy:{requestCount}\")\n}\n",
    )
    .expect("entry");

    let built = topaz()
        .args(["build", "--root"])
        .arg(&base)
        .arg("--out-dir")
        .arg(&output)
        .output()
        .expect("build runs");
    assert!(built.status.success(), "{built:?}");
    let binary = output
        .join("target/debug")
        .join(format!("program{}", std::env::consts::EXE_SUFFIX));
    let embedded_config = std::fs::read_to_string(output.join("topaz-service-config.json"))
        .expect("managed service configuration");
    assert!(
        embedded_config.contains("\"schema\": \"topaz.httpServiceConfig.v1\""),
        "{embedded_config}"
    );
    assert!(
        embedded_config.contains("\"source\": \"embedded-defaults\""),
        "{embedded_config}"
    );
    assert!(
        embedded_config.contains("\"values\": {"),
        "{embedded_config}"
    );
    let port = unused_loopback_port();
    let printed = Command::new(&binary)
        .args([
            "--port",
            &port.to_string(),
            "--workers",
            "2",
            "--print-config",
        ])
        .output()
        .expect("effective configuration prints");
    assert!(printed.status.success(), "{printed:?}");
    let effective = String::from_utf8_lossy(&printed.stdout);
    assert!(
        effective.contains("\"schema\": \"topaz.httpServiceConfig.v1\""),
        "{effective}"
    );
    assert!(
        effective.contains(&format!("\"port\": {port}")),
        "{effective}"
    );
    assert!(effective.contains("\"workers\": 2"), "{effective}");
    assert!(printed.stderr.is_empty(), "{printed:?}");
    let duplicate_control = Command::new(&binary)
        .args(["--print-config", "--print-config"])
        .output()
        .expect("duplicate control rejected");
    assert!(!duplicate_control.status.success(), "{duplicate_control:?}");
    assert!(
        String::from_utf8_lossy(&duplicate_control.stderr)
            .contains("duplicate service option `--print-config`"),
        "{duplicate_control:?}"
    );
    let mut service = Command::new(&binary)
        .args(["--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("service starts");

    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let spinner =
        std::thread::spawn(move || http_get_with_started(port, "/spin", Some(started_tx)));
    started_rx.recv().expect("spin request started");
    std::thread::sleep(std::time::Duration::from_millis(20));
    let overloaded = http_get(port, "/health");
    assert!(overloaded.starts_with("HTTP/1.1 503"), "{overloaded}");
    let timed_out = spinner.join().expect("timeout request joins");
    assert!(timed_out.starts_with("HTTP/1.1 504"), "{timed_out}");
    assert!(!timed_out.contains("private"), "{timed_out}");

    let invalid = http_get(port, "/invalid");
    assert!(invalid.starts_with("HTTP/1.1 500"), "{invalid}");
    assert!(!invalid.contains("private invalid response"), "{invalid}");
    let fault = http_get(port, "/fault");
    assert!(fault.starts_with("HTTP/1.1 500"), "{fault}");
    assert!(!fault.contains("division"), "{fault}");
    let oversized_response = http_get(port, "/large");
    assert!(
        oversized_response.starts_with("HTTP/1.1 500"),
        "{oversized_response}"
    );
    assert!(!oversized_response.contains("private-response"));
    let healthy = http_get(port, "/health");
    assert!(healthy.starts_with("HTTP/1.1 200"), "{healthy}");
    assert!(healthy.ends_with("healthy:1"), "{healthy}");
    let isolated = http_get(port, "/health");
    assert!(isolated.ends_with("healthy:1"), "{isolated}");

    let oversized_body = http_exchange(
        port,
        format!(
            "POST /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 17\r\nConnection: close\r\n\r\n12345678901234567"
        )
        .as_bytes(),
    );
    assert!(
        oversized_body.starts_with("HTTP/1.1 413"),
        "{oversized_body}"
    );
    let oversized_target = http_get(port, &format!("/{}", "x".repeat(8_193)));
    assert!(
        oversized_target.starts_with("HTTP/1.1 414"),
        "{oversized_target}"
    );

    let mut slow_body = service_connection(port);
    write!(
        slow_body,
        "POST /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Secret: SECRET-HEADER\r\nContent-Length: 4\r\nConnection: close\r\n\r\nS"
    )
    .expect("partial body write");
    let slow_body_response = bounded_socket_response(slow_body, 1_000);
    assert!(
        slow_body_response.starts_with("HTTP/1.1 408"),
        "{slow_body_response}"
    );

    let mut disconnected = service_connection(port);
    write!(
        disconnected,
        "POST /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 12\r\n\r\nSECRET-BODY"
    )
    .expect("disconnect body write");
    drop(disconnected);
    std::thread::sleep(std::time::Duration::from_millis(130));
    let after_disconnect = http_get(port, "/health");
    assert!(
        after_disconnect.ends_with("healthy:1"),
        "{after_disconnect}"
    );

    let mut slow_header_one = service_connection(port);
    slow_header_one
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1")
        .expect("first slow header");
    let mut slow_header_two = service_connection(port);
    slow_header_two
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1")
        .expect("second slow header");
    let overloaded_connection = service_connection(port);
    let overloaded_connection_response = bounded_socket_response(overloaded_connection, 1_000);
    assert!(
        overloaded_connection_response.is_empty()
            || overloaded_connection_response.starts_with("HTTP/1.1 503"),
        "{overloaded_connection_response}"
    );
    let slow_header_response = bounded_socket_response(slow_header_one, 1_000);
    assert!(
        slow_header_response.is_empty()
            || slow_header_response.starts_with("HTTP/1.1 400")
            || slow_header_response.starts_with("HTTP/1.1 408"),
        "{slow_header_response}"
    );
    drop(slow_header_two);

    let malformed = http_exchange(
        port,
        format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nBad Header\r\n\r\n").as_bytes(),
    );
    assert!(
        malformed.is_empty() || malformed.starts_with("HTTP/1.1 400"),
        "{malformed}"
    );
    let after_hostile = http_get(port, "/health");
    assert!(after_hostile.ends_with("healthy:1"), "{after_hostile}");

    service.kill().expect("service termination");
    let output = service.wait_with_output().expect("service reaped");
    let logs = String::from_utf8_lossy(&output.stderr);
    for private in [
        "SECRET-HEADER",
        "SECRET-BODY",
        "private invalid response",
        "private-response-17",
    ] {
        assert!(
            !logs.contains(private),
            "host logs leaked `{private}`: {logs}"
        );
    }
    for code in [
        "service-started",
        "handler-timeout",
        "invalid-response",
        "handler-fault",
        "request-rejected",
        "request-complete",
    ] {
        assert!(logs.contains(code), "host logs omitted `{code}`: {logs}");
    }
    for line in logs.lines().filter(|line| !line.trim().is_empty()) {
        assert!(
            line.starts_with("{\"schema\":\"topaz.httpServiceLog.v1\",\"requestId\":"),
            "host JSON log omitted its schema/correlation prefix: {line}"
        );
        assert!(line.ends_with('}'), "host JSON log is incomplete: {line}");
    }
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn web_app_lifecycle_mismatch_fails_before_wasm_build() {
    let dir = std::env::temp_dir().join("topaz_web_app_bad_lifecycle_test");
    let out_dir = dir.join("out");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(
        dir.join("topaz.toml"),
        r#"[package]
name = "bad_web_app"
version = "0.1.0"
language = "5.6"
entry = "src/main.tpz"

[build]
target = "web-app"
"#,
    )
    .expect("manifest");
    std::fs::write(
        dir.join("src/main.tpz"),
        "import std.dom { AppStep, BrowserEvent, Html, text }\n\n\
         export record Model { n: int }\n\
         export enum Msg { Tick }\n\
         export enum Other { Tick }\n\
         export function init() -> AppStep<Model, Msg> { AppStep { model: Model { n: 0 }, commands: [] } }\n\
         export function update(model: Model, message: Other, event: BrowserEvent) -> AppStep<Model, Msg> { AppStep { model: model, commands: [] } }\n\
         export function view(model: Model) -> Html<Msg> { text(\"bad\") }\n",
    )
    .expect("entry");
    let out = topaz()
        .args(["build", "--root"])
        .arg(&dir)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("invalid web-app lifecycle"),
        "{out:?}"
    );
    assert!(!out_dir.join("topaz-web.wasm").exists(), "{out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn web_app_v2_rejects_v1_step_and_event_before_wasm_build() {
    let dir = std::env::temp_dir().join("topaz_web_app_v2_bad_lifecycle_test");
    let out_dir = dir.join("out");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(
        dir.join("topaz.toml"),
        r#"[package]
name = "bad_web_app_v2"
version = "0.1.0"
language = "5.7"
entry = "src/main.tpz"

[build]
target = "web-app"

[web]
lifecycle = "v2"
"#,
    )
    .expect("manifest");
    std::fs::write(
        dir.join("src/main.tpz"),
        "import std.dom { AppStep, BrowserEvent, Html, text }\n\n\
         export record Model { n: int }\n\
         export enum Msg { Tick }\n\
         export function init() -> AppStep<Model, Msg> { AppStep { model: Model { n: 0 }, commands: [] } }\n\
         export function update(model: Model, message: Msg, event: BrowserEvent) -> AppStep<Model, Msg> { AppStep { model: model, commands: [] } }\n\
         export function view(model: Model) -> Html<Msg> { text(\"bad\") }\n",
    )
    .expect("entry");
    let out = topaz()
        .args(["build", "--root"])
        .arg(&dir)
        .arg("--out-dir")
        .arg(&out_dir)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("invalid web-app lifecycle"), "{stderr}");
    assert!(stderr.contains("WebAppStep"), "{stderr}");
    assert!(stderr.contains("lifecycle v2"), "{stderr}");
    assert!(!out_dir.join("topaz-web.wasm").exists(), "{out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
