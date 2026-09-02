use crate::*;

pub(super) const SELF_DIAGNOSTIC_CODE_PLACEHOLDER: &str = "TPZ0000";
pub(super) static INVOCATION_COMPILER_SELECTION: OnceLock<ResolvedCompilerSelection> =
    OnceLock::new();

pub(super) fn invocation_selection_origin() -> SelectionOrigin {
    let origin = INVOCATION_COMPILER_SELECTION
        .get()
        .and_then(|selection| selection.selection_origin);
    #[cfg(test)]
    if origin.is_none() {
        // Unit tests may call current-mode lowering helpers directly instead
        // of entering through `main`, where preflight records the real
        // invocation selection. The checked-in product never sees this
        // fallback; direct current-mode unit calls model the product default.
        return SelectionOrigin::CurrentDefault;
    }
    origin.expect("compiler-bearing invocation must record one selection origin")
}

pub(super) const USAGE: &str = "\
topaz — Topaz toolchain developer driver (run `topaz --version`)

USAGE:
    topaz <COMMAND> [ARGS]

COMMANDS:
    parse <file>      parse a Topaz source file and report diagnostics
    dump-ast <file>   parse a Topaz source file and print the AST
    check [entry]     parse + resolve + type a selected compilation unit
                      (unmarked entry = immutable 5.16; no entry = package
                      mode from topaz.toml)
    run [entry]       execute a program (unmarked entry = immutable 5.16;
                      no entry = package mode from topaz.toml);
                      --language-version 5.1 runs a single v5.1 file
    test [entry]      run a checked selected unit on the deterministic TestHost
                      (--root <package> keeps its manifest, lock, and dependencies
                      for a selected entry; no entry uses the manifest entry)
    fmt [entry]       parse-gated deterministic whitespace formatting
                      (`--check` reports drift without writing;
                      no entry = package mode; skips vendor/target)
    lsp               run the deterministic stdio language server; package
                      diagnostics use initialize.rootUri (or --root) with a
                      valid locked package and open-document overlays
    emit [entry]      expose generated Rust (default) or Python source for
                      inspection/integration (`--target rust|python`); print
                      the primary source to stdout, or write the complete
                      source set with --out-dir <dir> (--root <dir>;
                      no entry = package mode)
    build [entry]     lower + scaffold (--out-dir <dir>, required) + drive
                      cargo to build a native binary at
                      <dir>/target/<profile>/program (final binary only), or with
                      --target web a wasm package at <dir>/topaz-web.wasm
                      (--target web-worker also emits worker glue), or with
                      package-only --target web-app a complete static product,
                      package-only --target http-service a managed HTTP/1 service,
                      --target python a Python deployment source bundle at
                      <dir>/program.py
                      (--release for an optimized build; --run to execute
                      native builds after building; no entry = package mode)
    init              scaffold a deterministic current-profile package at --root
                      (default `.`); --target web-app creates the checked Web
                      Application scaffold and --target http-service creates a
                      checked HTTP handler; refuses to overwrite existing files
    dev               build and serve a package-mode web-app or http-service on 127.0.0.1
                      (web-app port 8000; service manifest default 8080;
                      optional --port and --out-dir)
    add <dep>         add a dependency to topaz.toml (`name@version`, or
                      `name --path <relative-path>` with content hash)
    lock              write deterministic topaz.lock for the package at
                      --root (default `.`); local/path dependency hashes must
                      already match topaz.toml
    fetch             fetch registry dependencies from --from <local-registry>
                      into vendor/<name>/<version>, verify content, write lock
    vendor            copy registry dependencies from --from <local-registry>
                      into vendor/<name>/<version>, verify content, write lock
    doc               generate deterministic package docs into --out-dir
    refactor rename <old> <new> [entry]
                      rename one unambiguous lexical binding in a single file
    refactor organize-imports [entry]
                      sort leading top-level single-line import blocks
    refactor add-missing-match-cases [entry]
                      add conservative enum match arms for TPZ5021 gaps
    refactor derive-json <file>:<line>
                      add JSON to a local record/enum derives clause
    migrate [entry]   migrate one supported language boundary and package metadata,
                      or check a source target without rewriting source
    bench [entry]     time the resolve + type-check pipeline without running
                      user code (no entry = package mode)
    compiler observe [entry]
                      write canonical observations through generated Rust source
                      to --out-dir (no entry = package mode)
    compiler preview [entry]
                      run the Topaz-authored front end through Rust Stage 0 and
                      write a typed-terminal observation to --out-dir;
                      add --producer stage1|stage2 --terminal rust-source for
                      an explicit compiler producer; Stage 2 also accepts
                      --self-source to compile the embedded compiler source set
    compiler validate <dir>
                      validate an observation bundle without recompiling
    compiler compare <left> <right>
                      compare two validated observations (semantic by default);
                      use --layer generated-source|provenance, or compare two
                      binary files with --layer native-binary
    compiler status   report the installed Rust/self compiler support contract
                      (`--json` emits topaz.compiler-support/v2)
    storage status    list Topaz-owned temporary build workspaces and sizes
    storage clean     remove inactive Topaz-owned temporary workspaces only
    lispex run <file> run raw Lispex source with the installed LIT companion
    lispex info       report the installed LIT component (--json required)
    lispex embed run  run one bounded exact embedded evaluator request
    lispex embed info report the exact embedded component (--json required)
    mcp serve         serve the installed Topaz reference, checker, and
                      no-capability runner over local stdio
    check-corpus      run the golden parse-corpus gates (repo checkout only)
    explain <TPZ####> explain a stable diagnostic code (--json for a
                      deterministic machine-readable object)
    version           print the toolchain version (--verbose for detail)
    license           print the Apache-2.0 license carried by this CLI
    notice            print Topaz and bundled third-party notices
    help              print this message

`run`, `emit`, and `build` statically type-check the unit first — the
same gate as `check` — so a type error stops them before anything runs
or is emitted. Pass --unchecked to skip that check and operate on the
program's runtime semantics directly (CDR-003 §13).

No-entry `check`/`run`/`emit`/`build`/`compiler observe`/`compiler preview` load topaz.toml from
--root (default `.`). Pass --locked in package mode to require topaz.lock's
root manifest_hash to match the current topaz.toml before compiling.

The current product identity is 5.20. Unmarked source remains on the immutable
5.16 profile (or topaz.toml's [package].language in package mode); pass
--language-version 5.20 to select the current profile explicitly.

OPTIONS:
    --language-version <5.1|5.2|5.3|5.4|5.5|5.6|5.7|5.8|5.9|5.10|5.11|5.12|5.13|5.14|5.15|5.16|5.17|5.18|5.19|5.20>
                                   language version selector
                                   (unmarked source 5.16; current identity
                                   5.20; manifest language wins in package
                                   mode; CDR-007)
    --compiler rust|self            compiler engine selector for admitted
                                   compiler-bearing commands (default self
                                   on supported current-mode routes);
                                   self is current-mode and never falls back
    --types                        accepted no-op: `check` types the
                                   unit by default (CDR-004 C-6)
    --format human|json            check only: diagnostic output format
                                   (default human; json = one JSON object
                                   per diagnostic, with stable fields)
    --profile agent-pack|test-profile|bootstrap
                                   check only: narrow canonical topaz-5.20 to
                                   the selected executable usage profile
    --exports-json                 check only: on a clean unit, print the
                                   checked public export surface as one
                                   deterministic JSON object
    --unchecked                    run/emit/build only: skip the default
                                   static type check and operate on the
                                   program's runtime semantics (CDR-003 §13)
    --root <dir>                   module-system root (SPEC v5.2 §17); the
                                   entry must be under it. Defaults to the
                                   entry file's own directory
    --out-dir <dir>                emit/build/compiler observe/compiler preview: write the
                                   managed output into <dir> (required)
    --layer semantic|generated-source|provenance|native-binary
                                   compiler compare: select one independent
                                   comparison layer (default semantic)
    --terminal ast|typed|rust-source
                                   compiler preview: ast or typed; compiler
                                   observe: typed or rust-source (defaults:
                                   preview=typed, observe=rust-source)
    --producer stage1|stage2       compiler preview only: select a fail-closed
                                   compiler producer (requires rust-source)
    --self-source                  Stage 2 compiler preview only: compile the
                                   embedded compiler source set
    --from <dir|version>           fetch/vendor: local registry root;
                                   migrate: source language version
    --to <version>                 migrate only: target language version
    --path <dir>                   add only: local dependency path
    --locked                       package-mode check/run/emit/build/test/doc/compiler
                                   observe: verify topaz.lock before compiling
    --check                        fmt only: report formatting drift without
                                   writing source or metadata
    --json                         compiler status/explain/bench/lispex info:
                                   render one deterministic machine-readable
                                   JSON object
    --backend boxed|native         emit/build: lowering backend (default boxed).
                                   `native` first tries whole-unit scalar Rust,
                                   then may specialize eligible top-level scalar
                                   functions inside the boxed application envelope.
                                   Unsupported shapes stay boxed. Checked builds
                                   only — not with `--unchecked`.
    --native-report-json <file>   checked native emit/build only: atomically
                                   write a deterministic lowering decision report
                                   without changing normal output or artifacts
    --target rust|python            emit only: generated source target
                                   (default rust)
    --target native|web|web-worker|web-app|http-service|python
                                   build only: native executable (default),
                                   v5.4 Web Target raw-wasm package,
                                   worker-ready web package, or Python bundle;
                                   package-mode Python builds are direct
                                   applications, while explicit-entry builds
                                   retain the trace integration contract
    -- <args...>                   run/build --run: explicit program args passed
                                   to exported main(args, stdin) (`test` also
                                   accepts them for checked TestHost runs)
    --verbose, -v                  compiler-bearing commands: report the
                                   resolved compiler and selection origin;
                                   version: print full detail
    --version, -V                  print the toolchain version
";

/// The lowering backend for `emit`/`build` (CDR-006 §4 / v5.4 native emit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Backend {
    /// The proven boxed tree-walking emitter (default; the only backend for
    /// `--unchecked` builds).
    Boxed,
    /// The v5.4 monomorphized native backend: lowers concrete-scalar islands to
    /// bare Rust over the shared checked-arith leaf, FALLING BACK to boxed for
    /// any shape it cannot guarantee byte-identical. Checked builds only.
    Native,
}

pub(super) struct NativeReportCapture {
    pub(super) command: &'static str,
    pub(super) target: String,
    pub(super) version: LangVersion,
    pub(super) decision: topaz_emit::NativeAttemptDecision,
}

pub(super) struct NativeReportSession {
    pub(super) destination: PathBuf,
    pub(super) temporary: PathBuf,
    pub(super) backup: PathBuf,
    pub(super) file: Option<fs::File>,
    pub(super) capture: Option<NativeReportCapture>,
    pub(super) duplicate_capture: bool,
    pub(super) active: bool,
}

impl NativeReportSession {
    pub(super) fn prepare(path: &str, out_dir: Option<&str>) -> Result<Self, String> {
        if path.is_empty() {
            return Err("`--native-report-json` requires a non-empty file path".to_string());
        }
        let cwd = std::env::current_dir()
            .map_err(|error| format!("cannot resolve current dir: {error}"))?;
        let destination = absolute_lexical_path(&cwd, Path::new(path));
        if let Some(out_dir) = out_dir.filter(|value| !value.is_empty()) {
            let managed = absolute_lexical_path(&cwd, Path::new(out_dir));
            if destination.starts_with(&managed) {
                return Err(format!(
                    "native report `{}` must be outside managed output `{}`",
                    destination.display(),
                    managed.display()
                ));
            }
        }
        if destination.is_dir() {
            return Err(format!(
                "native report destination `{}` is a directory",
                destination.display()
            ));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| "native report destination has no parent".to_string())?;
        if !parent.is_dir() {
            return Err(format!(
                "native report parent `{}` does not exist or is not a directory",
                parent.display()
            ));
        }
        let file_name = destination
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "native report destination needs a UTF-8 file name".to_string())?;
        let temporary = parent.join(format!(
            ".{file_name}.topaz-native-report-{}",
            std::process::id()
        ));
        let backup = parent.join(format!(
            ".{file_name}.topaz-native-report-backup-{}",
            std::process::id()
        ));
        if backup.exists() {
            return Err(format!(
                "stale native report backup `{}` exists",
                backup.display()
            ));
        }
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| {
                format!(
                    "cannot reserve native report `{}`: {error}",
                    destination.display()
                )
            })?;
        Ok(Self {
            destination,
            temporary,
            backup,
            file: Some(file),
            capture: None,
            duplicate_capture: false,
            active: true,
        })
    }

    pub(super) fn capture(
        &mut self,
        command: &'static str,
        target: impl Into<String>,
        version: LangVersion,
        decision: topaz_emit::NativeAttemptDecision,
    ) {
        if self.capture.is_some() {
            self.duplicate_capture = true;
            return;
        }
        self.capture = Some(NativeReportCapture {
            command,
            target: target.into(),
            version,
            decision,
        });
    }

    pub(super) fn finish(&mut self) -> Result<(), String> {
        if self.duplicate_capture {
            return Err("native lowering was captured more than once".to_string());
        }
        let capture = self
            .capture
            .as_ref()
            .ok_or_else(|| "native lowering completed without report facts".to_string())?;
        let json = render_native_report_json(capture);
        let mut file = self
            .file
            .take()
            .ok_or_else(|| "native report reservation is already closed".to_string())?;
        file.write_all(json.as_bytes()).map_err(|error| {
            format!(
                "cannot write native report `{}`: {error}",
                self.destination.display()
            )
        })?;
        file.sync_all().map_err(|error| {
            format!(
                "cannot sync native report `{}`: {error}",
                self.destination.display()
            )
        })?;
        drop(file);
        let had_destination = self.destination.exists();
        if had_destination {
            fs::rename(&self.destination, &self.backup).map_err(|error| {
                format!(
                    "cannot preserve previous native report `{}`: {error}",
                    self.destination.display()
                )
            })?;
        }
        if let Err(error) = fs::rename(&self.temporary, &self.destination) {
            if had_destination {
                let _ = fs::rename(&self.backup, &self.destination);
            }
            return Err(format!(
                "cannot install native report `{}`: {error}",
                self.destination.display()
            ));
        }
        if had_destination {
            let _ = fs::remove_file(&self.backup);
        }
        self.active = false;
        Ok(())
    }
}

impl Drop for NativeReportSession {
    fn drop(&mut self) {
        if self.active {
            let _ = self.file.take();
            let _ = fs::remove_file(&self.temporary);
        }
    }
}

pub(super) fn absolute_lexical_path(cwd: &Path, path: &Path) -> PathBuf {
    use std::path::Component;

    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

pub(super) fn render_native_report_json(capture: &NativeReportCapture) -> String {
    let decision = &capture.decision;
    let mut out = String::new();
    out.push_str("{\"schemaVersion\":\"topaz.native-lowering-report.v1\"");
    out.push_str(",\"toolchainVersion\":");
    push_json_string(&mut out, env!("CARGO_PKG_VERSION"));
    out.push_str(",\"languageMode\":");
    push_json_string(&mut out, &format!("topaz-{}", capture.version.as_str()));
    out.push_str(",\"command\":");
    push_json_string(&mut out, capture.command);
    out.push_str(",\"target\":");
    push_json_string(&mut out, &capture.target);
    out.push_str(",\"requestedBackend\":\"native\",\"selectedBackend\":");
    push_json_string(&mut out, decision.selected_backend);
    out.push_str(",\"selectionScope\":");
    push_json_string(&mut out, decision.selection_scope);
    out.push_str(",\"declineReason\":");
    match decision.decline_reason {
        Some(reason) => push_json_string(&mut out, reason),
        None => out.push_str("null"),
    }
    out.push_str(",\"declineDetail\":");
    match decision.decline_detail {
        Some(detail) => push_json_string(&mut out, detail),
        None => out.push_str("null"),
    }
    out.push_str(",\"entryModule\":");
    push_json_string(&mut out, &decision.entry_module);
    let _ = write!(out, ",\"moduleCount\":{}", decision.module_count);
    let _ = write!(out, ",\"containsExtern\":{}", decision.contains_extern);
    out.push_str(",\"functions\":[");
    for (index, function) in decision.functions.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"module\":");
        push_json_string(&mut out, &function.module);
        out.push_str(",\"path\":");
        push_json_string(&mut out, &function.path);
        out.push_str(",\"name\":");
        push_json_string(&mut out, &function.name);
        let _ = write!(
            out,
            ",\"span\":{{\"lo\":{},\"hi\":{}}}",
            function.span_lo, function.span_hi
        );
        out.push_str(",\"selectedBackend\":");
        push_json_string(&mut out, function.selected_backend);
        out.push_str(",\"selectionScope\":");
        push_json_string(&mut out, function.selection_scope);
        out.push_str(",\"declineReason\":");
        match function.decline_reason {
            Some(reason) => push_json_string(&mut out, reason),
            None => out.push_str("null"),
        }
        out.push_str(",\"declineDetail\":");
        match function.decline_detail {
            Some(detail) => push_json_string(&mut out, detail),
            None => out.push_str("null"),
        }
        out.push('}');
    }
    out.push_str("]}\n");
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EmitTarget {
    Rust,
    Python,
}

impl EmitTarget {
    pub(super) fn is_python(self) -> bool {
        matches!(self, Self::Python)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
        }
    }
}

/// Build product target. This is intentionally separate from the lowering
/// backend: `--backend native` means the monomorphized Rust emitter, while
/// `--target web`/`web-worker` means packaging the emitted Rust as a
/// wasm/browser artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuildTarget {
    Default,
    Native,
    Web,
    WebWorker,
    WebApp,
    HttpService,
    Python,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PreviewProducer {
    Stage1,
    Stage2,
}

impl BuildTarget {
    pub(super) fn is_web(self) -> bool {
        matches!(
            self,
            BuildTarget::Web | BuildTarget::WebWorker | BuildTarget::WebApp
        )
    }

    pub(super) fn is_python(self) -> bool {
        matches!(self, BuildTarget::Python)
    }

    pub(super) fn is_service(self) -> bool {
        matches!(self, BuildTarget::HttpService)
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            BuildTarget::Default => "default",
            BuildTarget::Native => "native",
            BuildTarget::Web => "web",
            BuildTarget::WebWorker => "web-worker",
            BuildTarget::WebApp => "web-app",
            BuildTarget::HttpService => "http-service",
            BuildTarget::Python => "python",
        }
    }

    pub(super) fn writes_worker(self) -> bool {
        matches!(self, BuildTarget::WebWorker)
    }
}

pub(super) fn manifest_build_target(value: &str) -> Result<BuildTarget, String> {
    match value {
        "native" => Ok(BuildTarget::Native),
        "web" => Ok(BuildTarget::Web),
        "web-worker" => Ok(BuildTarget::WebWorker),
        "web-app" => Ok(BuildTarget::WebApp),
        "http-service" => Ok(BuildTarget::HttpService),
        "python" => Ok(BuildTarget::Python),
        other => Err(format!(
            "topaz.toml [build].target `{other}` is unsupported (expected `native`, `web`, `web-worker`, `web-app`, `http-service`, or `python`)"
        )),
    }
}

pub(super) fn command_emit_target(
    command: Option<&str>,
    target: Option<&str>,
) -> Result<EmitTarget, String> {
    if command != Some("emit") {
        return Ok(EmitTarget::Rust);
    }
    match target {
        None | Some("rust") => Ok(EmitTarget::Rust),
        Some("python") => Ok(EmitTarget::Python),
        Some(value) => Err(format!(
            "unknown emit `--target` `{value}` (expected `rust` or `python`)"
        )),
    }
}

pub(super) fn command_build_target(
    command: Option<&str>,
    target: Option<&str>,
) -> Result<BuildTarget, String> {
    if command != Some("build") {
        return Ok(BuildTarget::Native);
    }
    match target {
        None => Ok(BuildTarget::Default),
        Some("native") => Ok(BuildTarget::Native),
        Some("web") => Ok(BuildTarget::Web),
        Some("web-worker") => Ok(BuildTarget::WebWorker),
        Some("web-app") => Ok(BuildTarget::WebApp),
        Some("http-service") => Ok(BuildTarget::HttpService),
        Some("python") => Ok(BuildTarget::Python),
        Some(value) => Err(format!(
            "unknown build `--target` `{value}` (expected `native`, `web`, `web-worker`, `web-app`, `http-service`, or `python`)"
        )),
    }
}

pub(super) fn compiler_preflight_uses_package_manifest(
    args: &[String],
    root: Option<&str>,
) -> bool {
    let rooted_package = root
        .map(Path::new)
        .is_some_and(|root| root.join("topaz.toml").is_file());
    match args {
        [command]
            if matches!(
                command.as_str(),
                "check" | "run" | "test" | "fmt" | "emit" | "build" | "dev" | "doc" | "bench"
            ) =>
        {
            true
        }
        [command] if command == "lsp" => rooted_package,
        [command, _] if command == "test" => rooted_package,
        [compiler, observe] if compiler == "compiler" && observe == "observe" => true,
        _ => false,
    }
}

pub(super) fn compiler_preflight_language_version(
    args: &[String],
    root: Option<&str>,
    version_arg: Option<LangVersion>,
    default_version: LangVersion,
    retain_nondeterministic: bool,
) -> Result<LangVersion, String> {
    if !compiler_preflight_uses_package_manifest(args, root) {
        return Ok(default_version);
    }
    let root = root.unwrap_or(".");
    // Selection reads only the root manifest. Dependency resolution, lock
    // verification, target source loading, cache lookup, and output mutation
    // remain after compiler selection.
    let project = if retain_nondeterministic {
        topaz_package::Project::load_for_profile(root)
    } else {
        topaz_package::Project::load(root)
    }
    .map_err(|error| error.to_string())?;
    let manifest_version = project.manifest.package.language;
    if let Some(selected) = version_arg
        && selected != manifest_version
    {
        return Err(format!(
            "--language-version conflicts with topaz.toml [package].language \
             (manifest {}, CLI {})",
            lang_version_text(manifest_version),
            lang_version_text(selected)
        ));
    }
    Ok(manifest_version)
}

pub(super) struct CommandFlags {
    pub(super) compiler_intent: CompilerIntent,
    pub(super) types: bool,
    pub(super) release: bool,
    pub(super) run: bool,
    pub(super) unchecked: bool,
    pub(super) experimental: bool,
    pub(super) locked: bool,
    pub(super) self_source: bool,
    pub(super) fmt_check: bool,
    pub(super) exports_json: bool,
    pub(super) json: bool,
    pub(super) verbose: bool,
    pub(super) version: bool,
    pub(super) root: Option<String>,
    pub(super) out_dir: Option<String>,
    pub(super) native_report: Option<String>,
    pub(super) comparison_layer_arg: Option<String>,
    pub(super) comparison_layer: topaz_kernel::ComparisonLayer,
    pub(super) observation_terminal_arg: Option<String>,
    pub(super) observation_terminal: topaz_kernel::TerminalPhase,
    pub(super) preview_terminal: topaz_kernel::TerminalPhase,
    pub(super) producer_arg: Option<String>,
    pub(super) preview_producer: Option<PreviewProducer>,
    pub(super) port: Option<String>,
    pub(super) from: Option<String>,
    pub(super) to: Option<String>,
    pub(super) path: Option<String>,
    pub(super) profile_arg: Option<String>,
    pub(super) check_profile: Option<profile::CheckProfile>,
    pub(super) format_arg: Option<String>,
    pub(super) json_format: bool,
    pub(super) backend_arg: Option<String>,
    pub(super) backend: Backend,
    pub(super) target: Option<String>,
}

impl CommandFlags {
    pub(super) fn take(args: &mut Vec<String>) -> Result<Self, String> {
        let compiler_arg = take_cli_value(args, "--compiler")?;
        let compiler_intent = match compiler_arg.as_deref() {
            None => CompilerIntent::Omitted,
            Some(value) => CompilerSelection::parse(value)
                .map(CompilerIntent::Explicit)
                .ok_or_else(|| {
                    format!("unknown `--compiler` `{value}` (expected `rust` or `self`)")
                })?,
        };
        let types = take_bool_flag(args, "--types");
        let release = take_bool_flag(args, "--release");
        let run = take_bool_flag(args, "--run");
        let unchecked = take_bool_flag(args, "--unchecked");
        let experimental = take_bool_flag(args, "--experimental");
        let locked = take_bool_flag(args, "--locked");
        let self_source = take_bool_flag(args, "--self-source");
        let fmt_check = take_bool_flag(args, "--check");
        let exports_json = take_bool_flag(args, "--exports-json");
        let json = take_bool_flag(args, "--json");
        let verbose = take_bool_flag(args, "--verbose") || take_bool_flag(args, "-v");
        let version = take_bool_flag(args, "--version") || take_bool_flag(args, "-V");
        let root = take_cli_value(args, "--root")?;
        let out_dir = take_cli_value(args, "--out-dir")?;
        let native_report = take_cli_value(args, "--native-report-json")?;
        let comparison_layer_arg = take_cli_value(args, "--layer")?;
        let observation_terminal_arg = take_cli_value(args, "--terminal")?;
        let producer_arg = take_cli_value(args, "--producer")?;
        let preview_producer = match producer_arg.as_deref() {
            None => None,
            Some("stage1") => Some(PreviewProducer::Stage1),
            Some("stage2") => Some(PreviewProducer::Stage2),
            Some(value) => {
                return Err(format!(
                    "unknown `--producer` `{value}` (expected `stage1` or `stage2`)"
                ));
            }
        };
        let observation_terminal = match observation_terminal_arg.as_deref() {
            None | Some("rust-source") => topaz_kernel::TerminalPhase::RustSource,
            Some("ast") => topaz_kernel::TerminalPhase::Ast,
            Some("typed") => topaz_kernel::TerminalPhase::Typed,
            Some(value) => {
                return Err(format!(
                    "unknown `--terminal` `{value}` (expected `ast`, `typed`, or `rust-source`)"
                ));
            }
        };
        let preview_terminal = observation_terminal_arg
            .as_ref()
            .map_or(topaz_kernel::TerminalPhase::Typed, |_| observation_terminal);
        let comparison_layer = match comparison_layer_arg.as_deref() {
            None => topaz_kernel::ComparisonLayer::Semantic,
            Some(value) => topaz_kernel::ComparisonLayer::parse(value).ok_or_else(|| {
                format!(
                    "unknown `--layer` `{value}` (expected `semantic`, `generated-source`, `provenance`, or `native-binary`)"
                )
            })?,
        };
        let port = take_cli_value(args, "--port")?;
        let from = take_cli_value(args, "--from")?;
        let to = take_cli_value(args, "--to")?;
        let path = take_cli_value(args, "--path")?;
        let profile_arg = take_cli_value(args, "--profile")?;
        let check_profile = match profile_arg.as_deref() {
            None => None,
            Some(value) => profile::CheckProfile::parse(value)
                .map(Some)
                .ok_or_else(|| {
                    format!(
                        "unknown `--profile` `{value}` (expected `agent-pack`, `test-profile`, or `bootstrap`)"
                    )
                })?,
        };
        let format_arg = take_cli_value(args, "--format")?;
        let json_format = match format_arg.as_deref() {
            None | Some("human") => false,
            Some("json") => true,
            Some(value) => {
                return Err(format!(
                    "unknown `--format` `{value}` (expected `human` or `json`)"
                ));
            }
        };
        let backend_arg = take_cli_value(args, "--backend")?;
        let backend = match backend_arg.as_deref() {
            None | Some("boxed") => Backend::Boxed,
            Some("native") => Backend::Native,
            Some(value) => {
                return Err(format!(
                    "unknown `--backend` `{value}` (expected `boxed` or `native`)"
                ));
            }
        };
        let target = take_cli_value(args, "--target")?;
        Ok(Self {
            compiler_intent,
            types,
            release,
            run,
            unchecked,
            experimental,
            locked,
            self_source,
            fmt_check,
            exports_json,
            json,
            verbose,
            version,
            root,
            out_dir,
            native_report,
            comparison_layer_arg,
            comparison_layer,
            observation_terminal_arg,
            observation_terminal,
            preview_terminal,
            producer_arg,
            preview_producer,
            port,
            from,
            to,
            path,
            profile_arg,
            check_profile,
            format_arg,
            json_format,
            backend_arg,
            backend,
            target,
        })
    }
}

pub(super) fn take_cli_value(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, String> {
    take_value_flag(args, flag).map_err(|()| format!("`{flag}` requires a value"))
}

pub(super) fn dispatch_protocol_entry(args: &[String]) -> Option<ExitCode> {
    if args == ["__topaz-mcp-worker"] {
        return Some(match topaz_mcp::run_worker_stdio() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("topaz MCP worker: {error}");
                ExitCode::FAILURE
            }
        });
    }
    if args.first().map(String::as_str) == Some("mcp") {
        if args != ["mcp", "serve"] {
            eprintln!("topaz mcp: expected exactly `serve`");
            return Some(ExitCode::FAILURE);
        }
        let executable = match std::env::current_exe() {
            Ok(executable) => executable,
            Err(error) => {
                eprintln!("topaz mcp serve: cannot locate installed Topaz executable: {error}");
                return Some(ExitCode::FAILURE);
            }
        };
        return Some(
            match topaz_mcp::run_stdio_server(executable, "self".to_string()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("topaz mcp serve: {error}");
                    ExitCode::FAILURE
                }
            },
        );
    }
    if args.first().map(String::as_str) == Some("lispex")
        && args.get(1).map(String::as_str) == Some("embed")
    {
        return Some(lispex_embed::dispatch(&args[2..]));
    }
    None
}
