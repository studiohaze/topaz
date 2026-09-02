use crate::*;

pub(super) fn dev_package_target(
    target: &PackageTarget,
    out_dir: Option<&str>,
    port_arg: Option<&str>,
    compiler_selection: CompilerSelection,
) -> ExitCode {
    let effective = match manifest_build_target(&target.build_target) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("topaz: {error}");
            return ExitCode::FAILURE;
        }
    };
    if effective == BuildTarget::HttpService {
        return dev_http_service(target, out_dir, port_arg, compiler_selection);
    }
    if effective != BuildTarget::WebApp {
        eprintln!(
            "topaz: `dev` requires a package whose [build].target is `web-app` or `http-service` (got `{}`)",
            target.build_target
        );
        return ExitCode::FAILURE;
    }
    let port = match port_arg.unwrap_or("8000").parse::<u16>() {
        Ok(port) if port != 0 => port,
        _ => {
            eprintln!("topaz: `--port` must be an integer from 1 through 65535");
            return ExitCode::FAILURE;
        }
    };
    let output = out_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| target.root.join(".topaz/dev/web-app"));
    if build_dev_product(compiler_selection, target, &output, BuildTarget::WebApp)
        != ExitCode::SUCCESS
    {
        return ExitCode::FAILURE;
    }
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("topaz: cannot bind loopback port {port}: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = listener.set_nonblocking(true) {
        eprintln!("topaz: cannot configure dev server: {error}");
        return ExitCode::FAILURE;
    }
    eprintln!("topaz: dev server listening on http://127.0.0.1:{port}/");
    let mut generation = 1_u64;
    let mut snapshot = project_input_snapshot(target);
    let mut pending_since: Option<SystemTime> = None;
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Err(error) = stream.set_nonblocking(false) {
                    eprintln!("topaz: cannot configure dev connection: {error}");
                    continue;
                }
                if let Err(error) = serve_dev_request(&mut stream, &output, generation) {
                    eprintln!("topaz: dev request failed: {error}");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => {
                eprintln!("topaz: dev server failed: {error}");
                return ExitCode::FAILURE;
            }
        }
        let next = project_input_snapshot(target);
        if next != snapshot {
            snapshot = next;
            pending_since = Some(SystemTime::now());
        }
        if pending_since.is_some_and(|started| {
            started.elapsed().unwrap_or_default() >= Duration::from_millis(180)
        }) {
            pending_since = None;
            eprintln!("topaz: project input changed; rebuilding …");
            if build_dev_product(compiler_selection, target, &output, BuildTarget::WebApp)
                == ExitCode::SUCCESS
            {
                generation += 1;
                eprintln!("topaz: rebuild succeeded; browser reload is ready");
            } else {
                eprintln!("topaz: rebuild failed; continuing to serve the last good product");
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

pub(super) fn dev_http_service(
    target: &PackageTarget,
    out_dir: Option<&str>,
    port_arg: Option<&str>,
    compiler_selection: CompilerSelection,
) -> ExitCode {
    let port = match port_arg
        .map(str::to_string)
        .unwrap_or_else(|| target.service.port.to_string())
        .parse::<u16>()
    {
        Ok(port) if port != 0 => port,
        _ => {
            eprintln!("topaz: `--port` must be an integer from 1 through 65535");
            return ExitCode::FAILURE;
        }
    };
    let output = out_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| target.root.join(".topaz/dev/http-service"));
    if build_dev_product(
        compiler_selection,
        target,
        &output,
        BuildTarget::HttpService,
    ) != ExitCode::SUCCESS
    {
        return ExitCode::FAILURE;
    }
    let binary = output
        .join("target/debug")
        .join(format!("program{}", std::env::consts::EXE_SUFFIX));
    let mut command = std::process::Command::new(&binary);
    command.args(["--bind", "127.0.0.1", "--port", &port.to_string()]);
    eprintln!("topaz: starting service development loop on http://127.0.0.1:{port}/");

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = command.exec();
        eprintln!("topaz: could not start `{}`: {error}", binary.display());
        ExitCode::FAILURE
    }
    #[cfg(not(unix))]
    {
        match command.status() {
            Ok(status) => ExitCode::from(u8::try_from(status.code().unwrap_or(1)).unwrap_or(1)),
            Err(error) => {
                eprintln!("topaz: could not start `{}`: {error}", binary.display());
                ExitCode::FAILURE
            }
        }
    }
}

pub(super) fn build_dev_product(
    compiler_selection: CompilerSelection,
    target: &PackageTarget,
    out_dir: &Path,
    build_target: BuildTarget,
) -> ExitCode {
    match compiler_selection {
        CompilerSelection::Rust => build_package_target(
            target,
            Some(out_dir),
            false,
            false,
            false,
            Backend::Boxed,
            false,
            build_target,
            false,
            &[],
            None,
        ),
        CompilerSelection::SelfHosted => {
            build_self_package_target(target, Some(out_dir), false, false, build_target, &[])
        }
    }
}

pub(super) fn project_input_snapshot(target: &PackageTarget) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut paths = vec![
        target.root.join("topaz.toml"),
        target.root.join("topaz.lock"),
    ];
    collect_topaz_sources(&target.root, &mut paths);
    for declared in target.web.styles.iter().chain(&target.web.assets) {
        collect_declared_web_inputs(&target.root.join(declared), &mut paths);
    }
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .filter_map(|path| {
            let metadata = fs::metadata(&path).ok()?;
            let modified = metadata.modified().ok()?;
            Some((path, metadata.len(), modified))
        })
        .collect()
}

pub(super) fn collect_topaz_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if matches!(
                entry.file_name().to_str(),
                Some(".git" | ".topaz" | "target" | "vendor" | "node_modules")
            ) {
                continue;
            }
            collect_topaz_sources(&path, out);
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "tpz") {
            out.push(path);
        }
    }
}

pub(super) fn collect_declared_web_inputs(path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    if !metadata.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        collect_declared_web_inputs(&entry.path(), out);
    }
}

pub(super) fn serve_dev_request(
    stream: &mut TcpStream,
    product_root: &Path,
    generation: u64,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut reader = std::io::BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("");
    let raw_path = parts.next().unwrap_or("");
    if !matches!(method, "GET" | "HEAD") {
        return write_http(
            stream,
            405,
            "text/plain; charset=utf-8",
            b"method not allowed",
            method == "HEAD",
        );
    }
    let path = raw_path.split('?').next().unwrap_or("");
    if path == "/__topaz_version" {
        return write_http(
            stream,
            200,
            "text/plain; charset=utf-8",
            generation.to_string().as_bytes(),
            method == "HEAD",
        );
    }
    if path.contains('%') || path.contains('\\') {
        return write_http(
            stream,
            400,
            "text/plain; charset=utf-8",
            b"bad path",
            method == "HEAD",
        );
    }
    let relative = path.trim_start_matches('/');
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    let candidate = Path::new(relative);
    if candidate
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return write_http(
            stream,
            400,
            "text/plain; charset=utf-8",
            b"bad path",
            method == "HEAD",
        );
    }
    let file = product_root.join(candidate);
    let canonical_file = match resolve_dev_file(product_root, candidate) {
        Ok(file) => file,
        Err(status) => {
            let body: &[u8] = if status == 400 {
                b"bad path"
            } else {
                b"not found"
            };
            return write_http(
                stream,
                status,
                "text/plain; charset=utf-8",
                body,
                method == "HEAD",
            );
        }
    };
    if !canonical_file.is_file() {
        return write_http(
            stream,
            404,
            "text/plain; charset=utf-8",
            b"not found",
            method == "HEAD",
        );
    }
    let bytes = fs::read(&canonical_file)?;
    write_http(stream, 200, web_mime(&file), &bytes, method == "HEAD")
}

pub(super) fn resolve_dev_file(product_root: &Path, relative: &Path) -> Result<PathBuf, u16> {
    let canonical_root = fs::canonicalize(product_root).map_err(|_| 404_u16)?;
    let canonical_file = fs::canonicalize(product_root.join(relative)).map_err(|_| 404_u16)?;
    if !canonical_file.starts_with(&canonical_root) || !canonical_file.is_file() {
        return Err(400);
    }
    Ok(canonical_file)
}

pub(super) fn web_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

pub(super) fn write_http(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    head: bool,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    if !head {
        stream.write_all(body)?;
    }
    stream.flush()
}
