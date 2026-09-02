use crate::*;

/// The pinned toolchain channel the closure was built against, parsed from the
/// embedded `rust-toolchain.toml` (CDR-006 §7) — e.g. `1.96.0`. The sanitized
/// build resolves that channel's cargo and rustc to absolute paths. `None` (a
/// malformed embedded pin) is fail-closed.
pub(super) fn pinned_channel() -> Option<&'static str> {
    VENDOR_TOOLCHAIN.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("channel")?.trim_start();
        let rest = rest.strip_prefix('=')?.trim_start().strip_prefix('"')?;
        Some(&rest[..rest.find('"')?])
    })
}

/// A CDR-006 §7 SANITIZED build context: a config-clean neutral CWD + a fresh
/// empty `CARGO_HOME` (both under a temp `root`), the pinned toolchain, and an
/// ABSOLUTE manifest/target, so cargo consumes EXACTLY the emitted tree's inputs
/// — no ambient `RUST*`/`CARGO_*` flags, wrappers, target-dir, registry, or
/// hierarchical `.cargo/config`. The platform linker/SDK env (`PATH`,
/// `RUSTUP_HOME`, MSVC `LIB`/`INCLUDE`, …) is inherited so the build can link.
pub(super) struct BuildEnv {
    pub(super) workspace: storage::Workspace,
    pub(super) cwd: PathBuf,
    pub(super) cargo_home: PathBuf,
    pub(super) manifest: PathBuf,
    pub(super) target: PathBuf,
    pub(super) log_dir: PathBuf,
    pub(super) cargo: PathBuf,
    pub(super) rustc: PathBuf,
}

impl BuildEnv {
    /// The exact pinned cargo with the exact pinned rustc, with the ambient
    /// build-influencing env stripped and only the emitted tree's own settings
    /// set. Absolute toolchain paths bypass PATH proxies and wrappers.
    pub(super) fn cargo(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(&self.cargo);
        // Cargo probes the target with `rustc -`, so inherited runner or caller
        // input can be parsed as Rust source. Every generated build is
        // non-interactive; give Cargo a dedicated EOF stdin while preserving
        // user input for the compiled program's later execution.
        cmd.current_dir(&self.cwd).stdin(Stdio::null());
        // Case-insensitive denylist (Windows env names are case-insensitive):
        // every CARGO_*, every RUSTC*/RUSTDOC*, RUSTFLAGS, and ambient
        // RUSTUP_TOOLCHAIN. RUSTUP_HOME / PATH / SDK vars are inherited.
        // `vars_os` (not `vars`) so a non-UTF-8 ambient env can't panic the build.
        for (k, _) in std::env::vars_os() {
            let u = k.to_string_lossy().to_ascii_uppercase();
            if u.starts_with("CARGO_")
                || u.starts_with("RUSTC")
                || u.starts_with("RUSTDOC")
                || u == "RUSTFLAGS"
                || u == "RUSTUP_TOOLCHAIN"
            {
                cmd.env_remove(&k);
            }
        }
        cmd.env("CARGO_HOME", &self.cargo_home)
            .env("CARGO_TARGET_DIR", &self.target)
            .env("CARGO_BUILD_JOBS", "1")
            .env("CARGO_INCREMENTAL", "0")
            .env("CARGO_NET_OFFLINE", "true")
            .env("RUSTC", &self.rustc);
        let emitted_config = self
            .manifest
            .parent()
            .map(|root| root.join(".cargo/config.toml"));
        if let Some(config) = emitted_config.filter(|path| path.is_file()) {
            // Cargo discovers config from the process CWD rather than an
            // absolute --manifest-path. The sanitized CWD is deliberately
            // neutral, so load only the emitted tree's audited source
            // replacement explicitly for service builds.
            cmd.arg("--config").arg(config);
        }
        let mut remap = std::ffi::OsString::from("--remap-path-prefix=");
        remap.push(self.workspace.root());
        remap.push("=/topaz-build");
        cmd.env("CARGO_ENCODED_RUSTFLAGS", remap);
        cmd
    }

    /// Remove the complete Topaz-owned workspace.
    pub(super) fn cleanup(&self) {
        self.workspace.cleanup();
    }

    /// The emitted crate's out-dir (the parent of its manifest) — where the
    /// captured cargo log is written.
    pub(super) fn out_dir(&self) -> &Path {
        &self.log_dir
    }
}

/// How a sanitized cargo invocation over the vendored closure failed (CDR-006
/// §7): an ENVIRONMENT-class failure (the user's machine — disk/linker/SDK/
/// toolchain/signal) is reclassified to the environment boundary with
/// remediation; anything else is an INTERNAL EMISSION ERROR (a Topaz compiler
/// defect, since the inputs are pinned + sanitized).
#[derive(Debug, PartialEq, Eq)]
pub(super) enum CargoFailure {
    Environment(&'static str),
    /// The active rustc is older than a dependency's `rust-version` (MSRV) — a
    /// user TOOLCHAIN problem, not a Topaz defect. Carries a version-named remedy.
    Msrv(String),
    Internal,
}

/// The narrow environment-token table: `(lowercase token, remediation)` for
/// linker/SDK discovery, missing rustup toolchain/target/std, disk, permission/
/// read-only FS, and OOM. Anything unrecognized is an internal emission error.
pub(super) const ENV_SIGNALS: &[(&str, &str)] = &[
    (
        "error: linking with",
        "a native linker failed — install a working C toolchain/linker",
    ),
    (
        "linker `",
        "the linker was not found — install a C toolchain (cc/clang or the MSVC tools)",
    ),
    (
        "cannot find -l",
        "a system library was not found — install the required SDK/dev libraries",
    ),
    (
        "= note: ld:",
        "the native linker reported an error — check the linker/SDK install",
    ),
    (
        "link.exe",
        "the MSVC linker failed — install the Visual Studio C++ build tools",
    ),
    (
        "xcrun",
        "the macOS command-line tools are missing — run `xcode-select --install`",
    ),
    ("no space left", "the disk is full — free space and retry"),
    (
        "permission denied",
        "a path was not writable — check permissions on the out-dir/target",
    ),
    (
        "read-only file system",
        "the target filesystem is read-only",
    ),
    (
        "is not installed",
        "a required rustup toolchain/target is not installed",
    ),
    (
        "can't find crate for `std`",
        "the target's std is not installed — `rustup target add <target>`",
    ),
    // cargo could not spawn the toolchain itself (`error: could not execute
    // process `rustc -vV``): the Rust toolchain is missing, broken, or not on
    // PATH. A user environment problem, NOT a Topaz emission defect. Anchored to
    // cargo's `error:` prefix so a path/progress line that merely contains the
    // phrase cannot trip it.
    (
        "error: could not execute process",
        "the Rust toolchain could not be run — install or repair it and ensure `rustc` is on PATH (https://rustup.rs)",
    ),
    ("out of memory", "the build ran out of memory"),
];

/// A rustc/cargo line that carries a DIAGNOSTIC, not an echoed source snippet.
/// Source/gutter lines (`5 | let x`, `   | ^^^`, `--> file`) echo USER code, so
/// scanning them for environment tokens would false-positive on a user string
/// like `"permission denied"`. Skip them; scan only the diagnostic lines.
pub(super) fn is_diagnostic_line(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with('|') || t.starts_with("-->") {
        return false;
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    !(digits > 0 && t[digits..].trim_start().starts_with('|'))
}

/// Detect cargo's MSRV refusal — `error: rustc <active> is not supported by the
/// following packages:` followed by `  <pkg>@<ver> requires rustc <required>` —
/// and return an actionable, version-named remedy. The active-too-old header is
/// REQUIRED (a bare `requires rustc` note is informational), so this never trips
/// on anything but a genuine toolchain-too-old failure. Scans only diagnostic
/// lines, like the rest of the classifier.
pub(super) fn detect_msrv(output: &str) -> Option<String> {
    let mut active: Option<&str> = None;
    let mut required: Option<&str> = None;
    for line in output.lines().filter(|l| is_diagnostic_line(l)) {
        let t = line.trim();
        // Format A (Cargo 1.96): `error: rustc <active> is not supported by the
        // following packages:` then `  <pkg> requires rustc <required>`.
        if active.is_none()
            && let Some(rest) = t.strip_prefix("error: rustc ")
            && let Some(idx) = rest.find(" is not supported")
        {
            active = Some(rest[..idx].trim());
        }
        // Format B (single line): `error: package `X` cannot be built because it
        // requires rustc <required> or newer, while the currently active rustc
        // version is <active>`.
        if active.is_none()
            && let Some(idx) = t.find("currently active rustc version is ")
        {
            let ver = t[idx + "currently active rustc version is ".len()..]
                .split_whitespace()
                .next()
                .unwrap_or("");
            if !ver.is_empty() {
                active = Some(ver.trim_end_matches('.'));
            }
        }
        if required.is_none()
            && let Some(idx) = t.find("requires rustc ")
        {
            let ver = t[idx + "requires rustc ".len()..]
                .split_whitespace()
                .next()
                .unwrap_or("");
            if !ver.is_empty() {
                required = Some(ver);
            }
        }
    }
    let active = active?;
    Some(match required {
        Some(r) => format!(
            "your active Rust toolchain (rustc {active}) is older than this build needs \
             (rustc {r}) — update it with `rustup update`, or select a rustc >= {r}"
        ),
        None => format!(
            "your active Rust toolchain (rustc {active}) is older than this build needs \
             — update it with `rustup update`"
        ),
    })
}

/// Classify FAILED cargo output: an MSRV (toolchain-too-old) refusal first, then
/// the narrow environment-token table, scanning only DIAGNOSTIC lines (not echoed
/// source snippets); unrecognized stays internal. Pure (unit-testable).
pub(super) fn classify_text(output: &str) -> CargoFailure {
    if let Some(remedy) = detect_msrv(output) {
        return CargoFailure::Msrv(remedy);
    }
    for line in output.lines().filter(|l| is_diagnostic_line(l)) {
        let lo = line.to_lowercase();
        for (tok, remedy) in ENV_SIGNALS {
            if lo.contains(tok) {
                return CargoFailure::Environment(remedy);
            }
        }
    }
    CargoFailure::Internal
}

/// Classify a FAILED sanitized cargo run. Checks the exit status/signal first (a
/// killed child can leave no useful text), then [`classify_text`] over combined
/// stdout+stderr.
pub(super) fn classify_cargo_failure(output: &std::process::Output) -> CargoFailure {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if output.status.signal().is_some() {
            return CargoFailure::Environment(
                "the build was terminated by a signal (out of memory, or interrupted)",
            );
        }
    }
    let mut blob = String::from_utf8_lossy(&output.stdout).into_owned();
    blob.push_str(&String::from_utf8_lossy(&output.stderr));
    classify_text(&blob)
}

#[cfg(test)]
pub(super) static TEST_CARGO_INVOCATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Run a sanitized cargo `phase` (lock/build), CAPTURE its output to
/// `<out-dir>/topaz-cargo-<phase>.log`, and on failure report a Topaz-owned
/// diagnostic (CDR-006 §7) — environment remediation, or an internal emission
/// error with a bug pointer + the log path — NEVER raw Rust as user-actionable.
pub(super) fn run_cargo_logged(
    env: &BuildEnv,
    phase: &str,
    mut cmd: std::process::Command,
) -> Result<(), ExitCode> {
    #[cfg(test)]
    let _test_cargo_guard = TEST_CARGO_INVOCATION_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("topaz: could not run cargo for `{phase}` ({e}); is rustup/cargo on PATH?");
            return Err(ExitCode::FAILURE);
        }
    };
    if output.status.success() {
        return Ok(());
    }
    let log = env.out_dir().join(format!("topaz-cargo-{phase}.log"));
    let mut blob = output.stdout.clone();
    blob.extend_from_slice(b"\n--- stderr ---\n");
    blob.extend_from_slice(&output.stderr);
    blob.extend_from_slice(format!("\n--- status: {} ---\n", output.status).as_bytes());
    let _ = fs::write(&log, &blob);
    match classify_cargo_failure(&output) {
        CargoFailure::Environment(remedy) => eprintln!(
            "topaz: the build environment failed during `{phase}` — {remedy}. Full log: {}",
            log.display()
        ),
        CargoFailure::Msrv(remedy) => eprintln!(
            "topaz: `{phase}` needs a newer Rust toolchain — {remedy}. Full log: {}",
            log.display()
        ),
        CargoFailure::Internal => eprintln!(
            "topaz: internal emission error during `{phase}` — the generated crate did not build, \
             which is a Topaz compiler defect (the inputs are pinned and sanitized), not something to \
             fix in the output. Please file a bug and attach this log: {}",
            log.display()
        ),
    }
    Err(ExitCode::FAILURE)
}

/// Why the build cannot be reproducibly ISOLATED from ambient cargo config, if
/// anything. Cargo's hierarchical config walk reads `.cargo/config[.toml]` from
/// every ancestor of the CWD (stable cargo has no flag to disable it), so on
/// UNIX isolation is refused if any ancestor (a) already holds a config that
/// would be loaded, (b) is group/other-writable, or (c) has a `.cargo/` that is
/// a symlink or group/other-writable — each a place a local race could plant a
/// `.cargo/config` after this check but before cargo reads. We proceed only when
/// none holds, so the check stays valid through the build.
///
/// On non-Unix (Windows), std exposes no portable ancestor-writability/ACL view
/// without platform crates (forbidden by the zero-dep policy); the cross-platform
/// config-PRESENCE check still applies, and Windows' default per-user temp is
/// ACL-private. Full Windows ACL race detection is a documented residual (1c).
pub(super) fn config_isolation_blocker(dir: &Path) -> Option<String> {
    let start = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut p: Option<&Path> = Some(&start);
    while let Some(d) = p {
        for name in ["config.toml", "config"] {
            let c = d.join(".cargo").join(name);
            if c.exists() {
                return Some(format!("an ambient `{}` would be loaded", c.display()));
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(m) = fs::metadata(d)
                && m.permissions().mode() & 0o022 != 0
            {
                return Some(format!(
                    "the ancestor `{}` is group/other-writable, so a local race could plant a \
                     `.cargo/config` there before the build reads it",
                    d.display()
                ));
            }
            // An existing `.cargo/` that is itself a symlink or other-writable is
            // a plantable target even if `d` is not writable.
            let cargo_dir = d.join(".cargo");
            if let Ok(m) = fs::symlink_metadata(&cargo_dir)
                && (m.file_type().is_symlink()
                    || (m.is_dir() && m.permissions().mode() & 0o022 != 0))
            {
                return Some(format!(
                    "the directory `{}` is a symlink or group/other-writable, so a `.cargo/config` \
                     could be planted in it",
                    cargo_dir.display()
                ));
            }
        }
        p = d.parent();
    }
    None
}

/// Probe that `dir` is writable by atomically CREATING-NEW a unique file (so it
/// cannot overwrite, or follow a symlink to clobber, an existing path), then
/// removing it.
pub(super) fn writable_probe(dir: &Path) -> std::io::Result<()> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let probe = dir.join(format!(".topaz-write-probe-{}-{nanos}", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)?;
    fs::remove_file(&probe)
}

/// Create a single directory, PRIVATE to this user (`0700` on Unix so an ambient
/// permissive umask can't leave it group/other-accessible), failing atomically
/// (EEXIST, including on a symlink) if the path already exists.
pub(super) fn create_private_dir(path: &Path) -> std::io::Result<()> {
    // `mut` is needed only on Unix, where `mode()` takes `&mut self`; on other
    // platforms the cfg block is empty so the binding is never mutated.
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut b = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(path)
}

/// Create a fresh, EXCLUSIVELY-OWNED, private temp directory: [`create_private_dir`]
/// fails atomically if the path already exists — including as a symlink — so the
/// sanitized `CARGO_HOME`/cwd cannot adopt an attacker-precreated directory in
/// the shared temp dir, and `0700` blocks a same-host race into it. Retried with
/// fresh entropy on collision.
pub(super) fn make_temp_root() -> std::io::Result<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    // Prefer a per-user PRIVATE base (`XDG_RUNTIME_DIR`, mode 0700 with non-
    // other-writable ancestors) so cargo's CWD-ancestor config walk has no
    // race-able directory; fall back to the system temp dir (on macOS already a
    // private per-user dir; on Linux `/tmp` is caught by the isolation check).
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    for _ in 0..32 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = base.join(format!("topaz-build-{}-{nanos}-{n}", std::process::id()));
        match create_private_dir(&root) {
            Ok(()) => return Ok(root),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not create a unique temp build dir",
    ))
}

/// Preflight the environment and build a SANITIZED [`BuildEnv`] for `out_dir`
/// (CDR-006 §7). Failures are Topaz-owned environment diagnostics with
/// remediation — the user's machine, the user's fix. On any failure the temp
/// `root` is removed so nothing leaks.
pub(super) fn prepare_build_env(out_dir: &Path) -> Result<BuildEnv, ExitCode> {
    let out_abs = fs::canonicalize(out_dir)
        .map(storage::command_path)
        .map_err(|e| {
            eprintln!("topaz: cannot access out-dir `{}`: {e}", out_dir.display());
            ExitCode::FAILURE
        })?;
    let (_channel, cargo, rustc) = validate_build_toolchain()?;
    let workspace = storage::Workspace::create().map_err(|e| {
        eprintln!("topaz: cannot create a sanitized build workspace: {e}");
        ExitCode::FAILURE
    })?;
    prepare_build_env_with_workspace(workspace, out_abs.clone(), out_abs, cargo, rustc)
}

pub(super) fn rustup_toolchain_binary(channel: &str, binary: &str) -> Option<PathBuf> {
    let output = std::process::Command::new("rustup")
        .args(["which", "--toolchain", channel, binary])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
    path.is_absolute().then_some(path)
}

pub(super) fn validate_build_toolchain() -> Result<(&'static str, PathBuf, PathBuf), ExitCode> {
    let channel = match pinned_channel() {
        Some(c) => c,
        None => {
            eprintln!(
                "topaz: internal error — the embedded toolchain pin is unreadable; please file a bug"
            );
            return Err(ExitCode::FAILURE);
        }
    };
    // rustup present? The sanitized build resolves the pinned cargo and rustc
    // through rustup, then invokes those absolute paths directly.
    let rustup_ok = std::process::Command::new("rustup")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !rustup_ok {
        eprintln!("topaz: `rustup` was not found — install Rust (https://rustup.rs), then retry");
        return Err(ExitCode::FAILURE);
    }
    let cargo = rustup_toolchain_binary(channel, "cargo");
    let rustc = rustup_toolchain_binary(channel, "rustc");
    let tc_ok = cargo
        .as_ref()
        .zip(rustc.as_ref())
        .map(|(cargo, rustc)| {
            std::process::Command::new(cargo)
                .arg("--version")
                .env("RUSTC", rustc)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        })
        .unwrap_or(false);
    if !tc_ok {
        eprintln!(
            "topaz: the pinned Rust toolchain `{channel}` is not available — run `rustup toolchain install {channel}`"
        );
        return Err(ExitCode::FAILURE);
    }
    Ok((channel, cargo.unwrap(), rustc.unwrap()))
}

pub(super) fn prepare_workspace_build_env(
    workspace: storage::Workspace,
    log_dir: &Path,
) -> Result<BuildEnv, ExitCode> {
    let source = fs::canonicalize(&workspace.source)
        .map(storage::command_path)
        .map_err(|e| {
            eprintln!("topaz: cannot access temporary source workspace: {e}");
            ExitCode::FAILURE
        })?;
    let log_dir = fs::canonicalize(log_dir)
        .map(storage::command_path)
        .map_err(|e| {
            eprintln!("topaz: cannot access out-dir `{}`: {e}", log_dir.display());
            ExitCode::FAILURE
        })?;
    let (_channel, cargo, rustc) = validate_build_toolchain()?;
    prepare_build_env_with_workspace(workspace, source, log_dir, cargo, rustc)
}

pub(super) fn prepare_build_env_with_workspace(
    workspace: storage::Workspace,
    out_abs: PathBuf,
    log_dir: PathBuf,
    cargo: PathBuf,
    rustc: PathBuf,
) -> Result<BuildEnv, ExitCode> {
    let cwd = workspace.cwd.clone();
    let cargo_home = workspace.cargo_home.clone();
    if let Some(reason) = config_isolation_blocker(&cwd) {
        eprintln!(
            "topaz: cannot isolate the build — {reason}; cargo offers no way to disable hierarchical \
             config discovery. Set TMPDIR or XDG_RUNTIME_DIR to a private (0700, non-other-writable) \
             directory and retry"
        );
        workspace.cleanup();
        return Err(ExitCode::FAILURE);
    }
    let target = workspace.target.clone();
    let writable = writable_probe(&out_abs).and_then(|_| writable_probe(&target));
    if let Err(e) = writable {
        eprintln!("topaz: build workspace is not writable: {e}");
        workspace.cleanup();
        return Err(ExitCode::FAILURE);
    }
    Ok(BuildEnv {
        workspace,
        cwd,
        cargo_home,
        manifest: out_abs.join("Cargo.toml"),
        target,
        log_dir,
        cargo,
        rustc,
    })
}

/// Generate the version-exact `Cargo.lock` the emitted tree CARRIES (CDR-006 §7),
/// so `topaz build` can run `--offline --locked`. Runs in the sanitized env; the
/// path-only / zero-external closure means `--offline` fetches nothing.
pub(super) fn generate_lockfile(env: &BuildEnv) -> Result<(), ExitCode> {
    let mut cmd = env.cargo();
    cmd.args(["generate-lockfile", "--offline", "--manifest-path"])
        .arg(&env.manifest);
    run_cargo_logged(env, "lock", cmd)
}
