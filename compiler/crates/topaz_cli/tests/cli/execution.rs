use super::support::*;

#[test]
fn test_command_runs_checked_unit_on_test_host() {
    let dir = std::env::temp_dir().join("topaz_test_command_single_file");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let ok = dir.join("ok.tpz");
    let bad = dir.join("bad.tpz");
    std::fs::write(&ok, "print(\"from test\")\nassert(true)\n").expect("ok src");
    std::fs::write(&bad, "assert(false, \"boom\")\n").expect("bad src");

    let out = rust_topaz()
        .arg("test")
        .arg(&ok)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("from test"), "{stdout}");
    assert!(stdout.contains("test-ok"), "{stdout}");

    let out = rust_topaz()
        .arg("test")
        .arg(&bad)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("TPZ4007"), "{stderr}");
    assert!(stderr.contains("test failed"), "{stderr}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn package_mode_test_uses_locked_manifest_entry() {
    let dir = std::env::temp_dir().join("topaz_package_test_command");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("mkdir");
    let manifest = package_manifest();
    std::fs::write(dir.join("topaz.toml"), &manifest).expect("manifest");
    std::fs::write(dir.join("topaz.lock"), package_lock(&manifest)).expect("lock");
    std::fs::write(
        dir.join("src/main.tpz"),
        "print(\"package test ok\")\nassert(true)\n",
    )
    .expect("entry");

    let out = topaz()
        .current_dir(&dir)
        .arg("test")
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("package test ok"), "{stdout}");
    assert!(stdout.contains("test-ok"), "{stdout}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn package_mode_test_accepts_a_locked_selected_test_entry() {
    let dir = std::env::temp_dir().join("topaz_package_selected_test_command");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src mkdir");
    std::fs::create_dir_all(dir.join("tests")).expect("tests mkdir");
    let manifest = package_manifest();
    std::fs::write(dir.join("topaz.toml"), &manifest).expect("manifest");
    std::fs::write(dir.join("topaz.lock"), package_lock(&manifest)).expect("lock");
    std::fs::write(dir.join("src/main.tpz"), "print(\"manifest entry\")\n").expect("entry");
    std::fs::write(
        dir.join("src/helpers.tpz"),
        "export function answer() -> int { 42 }\n",
    )
    .expect("helper");
    std::fs::write(
        dir.join("tests/unit.tpz"),
        "import src.helpers { answer }\nprint(\"selected test {answer()}\")\nassert(answer() == 42)\n",
    )
    .expect("test entry");

    let out = topaz()
        .arg("test")
        .arg("tests/unit.tpz")
        .arg("--root")
        .arg(&dir)
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("selected test 42"), "{stdout}");
    assert!(stdout.contains("tests/unit.tpz: test-ok"), "{stdout}");
    assert!(!stdout.contains("manifest entry"), "{stdout}");

    let outside = dir.with_extension("outside.tpz");
    std::fs::write(&outside, "assert(true)\n").expect("outside entry");
    let escaped = topaz()
        .arg("test")
        .arg(&outside)
        .arg("--root")
        .arg(&dir)
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(!escaped.status.success(), "{escaped:?}");
    assert!(
        String::from_utf8_lossy(&escaped.stderr).contains("must be a `.tpz` file inside"),
        "{escaped:?}"
    );

    let _ = std::fs::remove_file(outside);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn package_mode_run_enforces_manifest_fs_capabilities() {
    let dir = std::env::temp_dir().join("topaz_package_fs_caps_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src mkdir");
    std::fs::create_dir_all(dir.join("data")).expect("data mkdir");
    std::fs::create_dir_all(dir.join("out")).expect("out mkdir");
    std::fs::write(dir.join("data/input.txt"), "read-ok").expect("read seed");
    std::fs::write(dir.join("out/result.txt"), "old").expect("write seed");
    std::fs::write(dir.join("secret.txt"), "secret").expect("secret seed");
    let manifest = r#"[package]
name = "pkg_mode"
version = "0.1.0"
language = "5.16"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.16"

[capabilities.fs]
read = ["data"]
write = ["out"]
"#;
    std::fs::write(dir.join("topaz.toml"), manifest).expect("manifest");
    std::fs::write(dir.join("topaz.lock"), package_lock(manifest)).expect("lock");
    std::fs::write(
        dir.join("src/main.tpz"),
        r#"import std.fs

let allowed = match fs.readText("data/input.txt") {
  case Ok(text) => text
  case Err(e) => e
}
let wrote = match fs.writeText("out/result.txt", "write-ok") {
  case Ok(_) => "write-ok"
  case Err(e) => e
}
let denied = match fs.readText("secret.txt") {
  case Ok(text) => "leak:{text}"
  case Err(e) => e
}
print("{allowed}/{wrote}/{denied}")
"#,
    )
    .expect("entry");

    let out = topaz()
        .current_dir(&dir)
        .arg("run")
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("read-ok/write-ok/"), "{stdout}");
    assert!(stdout.contains("not permitted"), "{stdout}");
    assert_eq!(
        std::fs::read_to_string(dir.join("out/result.txt")).expect("written"),
        "write-ok"
    );

    std::fs::write(dir.join("out/result.txt"), "old").expect("reset write seed");
    let build_dir = dir.join("build-out");
    let out = topaz()
        .current_dir(&dir)
        .arg("build")
        .arg("--locked")
        .arg("--out-dir")
        .arg(&build_dir)
        .arg("--run")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("read-ok/write-ok/"), "{stdout}");
    assert!(stdout.contains("not permitted"), "{stdout}");
    assert_eq!(
        std::fs::read_to_string(dir.join("out/result.txt")).expect("written by build"),
        "write-ok"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(target_os = "linux")]
#[test]
fn package_mode_run_preserves_non_unicode_physical_fs_capability_root() {
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "topaz_native_host_non_unicode_root_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("test root");
    let physical = root.join(std::ffi::OsString::from_vec(b"package-\xff".to_vec()));
    std::fs::create_dir_all(physical.join("src")).expect("physical source root");
    std::fs::create_dir_all(physical.join("data")).expect("physical capability root");
    std::fs::write(physical.join("data/input.txt"), "exact native path").expect("capability input");
    std::fs::write(
        physical.join("topaz.toml"),
        r#"[package]
name = "native_host_exact_path"
version = "0.1.0"
language = "5.19"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.19"

[capabilities.fs]
read = ["data"]
write = []
"#,
    )
    .expect("package manifest");
    std::fs::write(
        physical.join("src/main.tpz"),
        r#"import std.fs

let value = match fs.readText("data/input.txt") {
  case Ok(text) => text
  case Err(error) => error
}
print(value)
"#,
    )
    .expect("entry source");
    let logical = root.join("package");
    symlink(&physical, &logical).expect("logical package symlink");

    let out = rust_topaz()
        .arg("run")
        .arg("--root")
        .arg(&logical)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "exact native path\n");

    let _ = std::fs::remove_dir_all(root);
}

#[cfg(target_os = "linux")]
#[test]
fn package_mode_run_rejects_non_unicode_fs_list_entry_name() {
    use std::os::unix::ffi::OsStringExt;

    let root = std::env::temp_dir().join(format!(
        "topaz_native_host_non_unicode_list_entry_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("source root");
    std::fs::create_dir_all(root.join("data")).expect("capability root");
    std::fs::write(
        root.join("data")
            .join(std::ffi::OsString::from_vec(b"entry-\xff".to_vec())),
        "unrepresentable name",
    )
    .expect("non-Unicode directory entry");
    std::fs::write(
        root.join("topaz.toml"),
        r#"[package]
name = "native_host_exact_list_name"
version = "0.1.0"
language = "5.19"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.19"

[capabilities.fs]
read = ["data"]
write = []
"#,
    )
    .expect("package manifest");
    std::fs::write(
        root.join("src/main.tpz"),
        r#"import std.fs

let outcome = match fs.list("data") {
  case Ok(_) => "unexpected list success"
  case Err(error) => error
}
print(outcome)
"#,
    )
    .expect("entry source");

    let out = rust_topaz()
        .arg("run")
        .arg("--root")
        .arg(&root)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "cannot list `data`: directory entry name is not valid Unicode\n"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn built_native_and_python_packages_are_source_free_and_use_runtime_fs_capabilities() {
    let Some(python) = python_311_or_newer() else {
        eprintln!("skipping Python deployment test: Python 3.11+ is unavailable");
        return;
    };
    let base = std::env::temp_dir().join(format!(
        "topaz_relocatable_package_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    let app = base.join("app");
    std::fs::create_dir_all(app.join("src")).expect("src mkdir");
    std::fs::create_dir_all(app.join("data")).expect("data mkdir");
    std::fs::create_dir_all(app.join("out")).expect("out mkdir");
    std::fs::write(app.join("data/input.txt"), "physical").expect("read seed");
    std::fs::write(app.join("out/result.txt"), "old").expect("write seed");
    std::fs::write(app.join("secret.txt"), "secret").expect("secret seed");
    let manifest = r#"[package]
name = "relocatable_app"
version = "0.1.0"
language = "5.6"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.6"

[capabilities.fs]
read = ["data"]
write = ["out"]
"#;
    std::fs::write(app.join("topaz.toml"), manifest).expect("manifest");
    std::fs::write(
        app.join("src/main.tpz"),
        r#"import std.fs

export function main(args: Array<string>, stdin: string) -> Result<int, string> {
  if Cli.hasFlag(args, "--secret") {
    return match fs.readText("secret.txt") {
      case Ok(_) => Err("secret leaked")
      case Err(e) => Err(e)
    }
  }
  if Cli.hasFlag(args, "--link") {
    return match fs.readText("data/link.txt") {
      case Ok(_) => Err("symlink leaked")
      case Err(e) => Err(e)
    }
  }
  let entries = fs.list("data")?
  if entries.length == 0 {
    return Err("data directory is empty")
  }
  let input = match fs.readText("data/input.txt") {
    case Ok(text) => text
    case Err(e) => return Err(e)
  }
  match fs.writeText("out/result.txt", "written:{input}") {
    case Err(e) => Err(e)
    case Ok(_) => {
      print("{input}:{stdin}")
      Ok(0)
    }
  }
}
"#,
    )
    .expect("entry");

    let lock = topaz()
        .current_dir(&app)
        .arg("lock")
        .output()
        .expect("lock runs");
    assert!(lock.status.success(), "{lock:?}");

    let native_out = base.join("native-out");
    let native_build = topaz()
        .current_dir(&app)
        .arg("build")
        .arg("--locked")
        .arg("--out-dir")
        .arg(&native_out)
        .output()
        .expect("native build runs");
    assert!(native_build.status.success(), "{native_build:?}");

    let python_out = base.join("python-out");
    let python_build = topaz()
        .current_dir(&app)
        .arg("build")
        .arg("--target")
        .arg("python")
        .arg("--locked")
        .arg("--out-dir")
        .arg(&python_out)
        .output()
        .expect("Python build runs");
    assert!(python_build.status.success(), "{python_build:?}");

    let source_path = app.to_string_lossy();
    let native_artifact = native_out
        .join("target/debug")
        .join(format!("program{}", std::env::consts::EXE_SUFFIX));
    let native_bytes = std::fs::read(&native_artifact).expect("native artifact");
    assert!(
        !native_bytes
            .windows(source_path.len())
            .any(|window| window == source_path.as_bytes()),
        "native artifact embeds source package path"
    );
    let python_program = std::fs::read(python_out.join("program.py")).expect("Python artifact");
    assert!(
        !python_program
            .windows(source_path.len())
            .any(|window| window == source_path.as_bytes()),
        "Python artifact embeds source package path"
    );

    let native_runtime = base.join("native-runtime");
    let python_runtime = base.join("python-runtime");
    for runtime in [&native_runtime, &python_runtime] {
        std::fs::create_dir_all(runtime.join("data")).expect("runtime data");
        std::fs::create_dir_all(runtime.join("out")).expect("runtime out");
        std::fs::write(runtime.join("data/input.txt"), "physical").expect("runtime input");
        std::fs::write(runtime.join("out/result.txt"), "old").expect("runtime output seed");
        std::fs::write(runtime.join("secret.txt"), "secret").expect("runtime secret");
    }
    let runtime_binary = native_runtime.join(format!("program{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(&native_artifact, &runtime_binary).expect("copy native artifact");
    std::fs::copy(
        python_out.join("program.py"),
        python_runtime.join("program.py"),
    )
    .expect("copy Python program");
    std::fs::copy(
        python_out.join("topaz_py_rt.py"),
        python_runtime.join("topaz_py_rt.py"),
    )
    .expect("copy Python runtime");
    std::fs::rename(&app, base.join("source-removed")).expect("remove source package path");

    let mut native = Command::new(&runtime_binary);
    native.current_dir(&native_runtime);
    let native = output_with_stdin(native, b"pipe");
    assert_eq!(native.status.code(), Some(0), "{native:?}");

    let mut python_run = Command::new(&python);
    python_run.arg("program.py").current_dir(&python_runtime);
    let python_run = output_with_stdin(python_run, b"pipe");
    assert_eq!(python_run.status.code(), Some(0), "{python_run:?}");
    assert_eq!(python_run.stdout, native.stdout, "backend stdout differs");
    assert_eq!(native.stdout, b"physical:pipe\n");
    assert_eq!(
        std::fs::read(native_runtime.join("out/result.txt")).expect("native output"),
        std::fs::read(python_runtime.join("out/result.txt")).expect("Python output")
    );
    assert_eq!(
        std::fs::read_to_string(native_runtime.join("out/result.txt")).expect("output text"),
        "written:physical"
    );

    let native_denied = Command::new(&runtime_binary)
        .arg("--secret")
        .current_dir(&native_runtime)
        .output()
        .expect("native denied run");
    assert_eq!(native_denied.status.code(), Some(1), "{native_denied:?}");
    assert!(
        String::from_utf8_lossy(&native_denied.stderr).contains("not permitted"),
        "{native_denied:?}"
    );
    let python_denied = Command::new(&python)
        .arg("program.py")
        .arg("--secret")
        .current_dir(&python_runtime)
        .output()
        .expect("Python denied run");
    assert_eq!(python_denied.status.code(), Some(1), "{python_denied:?}");
    assert!(
        String::from_utf8_lossy(&python_denied.stderr).contains("not permitted"),
        "{python_denied:?}"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        symlink("../secret.txt", native_runtime.join("data/link.txt"))
            .expect("native symlink probe");
        symlink("../secret.txt", python_runtime.join("data/link.txt"))
            .expect("Python symlink probe");
        let native_link = Command::new(&runtime_binary)
            .arg("--link")
            .current_dir(&native_runtime)
            .output()
            .expect("native symlink run");
        assert_eq!(native_link.status.code(), Some(1), "{native_link:?}");
        assert!(
            String::from_utf8_lossy(&native_link.stderr)
                .contains("outside package fs capabilities"),
            "{native_link:?}"
        );
        let python_link = Command::new(&python)
            .arg("program.py")
            .arg("--link")
            .current_dir(&python_runtime)
            .output()
            .expect("Python symlink run");
        assert_eq!(python_link.status.code(), Some(1), "{python_link:?}");
        assert!(
            String::from_utf8_lossy(&python_link.stderr).contains("not permitted"),
            "{python_link:?}"
        );
    }

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn package_mode_explicit_main_receives_args_and_stdin() {
    let dir = std::env::temp_dir().join("topaz_package_explicit_main_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src mkdir");
    let manifest = r#"[package]
name = "pkg_mode"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.4"
"#;
    std::fs::write(dir.join("topaz.toml"), manifest).expect("manifest");
    std::fs::write(dir.join("topaz.lock"), package_lock(manifest)).expect("lock");
    std::fs::write(
        dir.join("src/main.tpz"),
        r#"export function main(args: Array<string>, stdin: string) -> Result<int, string> {
  print("{args.length}:{args[0]}:{stdin}")
  return Ok(7)
}
"#,
    )
    .expect("entry");

    let mut run = topaz();
    run.current_dir(&dir)
        .arg("run")
        .arg("--locked")
        .arg("--")
        .arg("--flag");
    let out = output_with_stdin(run, b"pipe");
    assert_eq!(out.status.code(), Some(7), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("1:--flag:pipe"),
        "{out:?}"
    );

    let mut test = topaz();
    test.current_dir(&dir)
        .arg("test")
        .arg("--locked")
        .arg("--")
        .arg("--flag");
    let out = output_with_stdin(test, b"pipe");
    assert_eq!(out.status.code(), Some(7), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("1:--flag:pipe"),
        "{out:?}"
    );

    let build_dir = dir.join("build-out");
    let mut build = topaz();
    build
        .current_dir(&dir)
        .arg("build")
        .arg("--locked")
        .arg("--out-dir")
        .arg(&build_dir)
        .arg("--run")
        .arg("--")
        .arg("--flag");
    let out = output_with_stdin(build, b"pipe");
    assert_eq!(out.status.code(), Some(7), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("1:--flag:pipe"),
        "{out:?}"
    );

    let native_build_dir = dir.join("build-native-out");
    let mut native_build = topaz();
    native_build
        .current_dir(&dir)
        .arg("build")
        .arg("--locked")
        .arg("--backend")
        .arg("native")
        .arg("--out-dir")
        .arg(&native_build_dir)
        .arg("--run")
        .arg("--")
        .arg("--flag");
    let out = output_with_stdin(native_build, b"pipe");
    assert_eq!(out.status.code(), Some(7), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("1:--flag:pipe"),
        "{out:?}"
    );

    let current_manifest = manifest
        .replace("language = \"5.4\"", "language = \"5.19\"")
        .replace("std = \"5.4\"", "std = \"5.19\"");
    std::fs::write(dir.join("topaz.toml"), &current_manifest).expect("current manifest");
    std::fs::write(dir.join("topaz.lock"), package_lock(&current_manifest)).expect("current lock");
    let mut self_test = topaz();
    self_test
        .current_dir(&dir)
        .arg("test")
        .arg("--locked")
        .args(["--compiler", "self"])
        .arg("--")
        .arg("--flag");
    let out = output_with_stdin(self_test, b"pipe");
    assert_eq!(out.status.code(), Some(7), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("1:--flag:pipe"),
        "{out:?}"
    );

    std::fs::write(
        dir.join("src/main.tpz"),
        "print(\"unexpected-mainless-execution\")\n",
    )
    .expect("mainless entry");
    let assert_mainless_args_rejected = |out: &std::process::Output| {
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("unexpected-mainless-execution"),
            "{out:?}"
        );
        assert!(
            String::from_utf8_lossy(&out.stderr)
                .contains("`--` program args require an exported `main(args, stdin)`"),
            "{out:?}"
        );
    };

    let mut mainless_run = topaz();
    mainless_run
        .current_dir(&dir)
        .arg("run")
        .arg("--locked")
        .arg("--")
        .arg("--flag");
    assert_mainless_args_rejected(&mainless_run.output().expect("mainless package run"));

    let mut mainless_test = topaz();
    mainless_test
        .current_dir(&dir)
        .arg("test")
        .arg("--locked")
        .arg("--")
        .arg("--flag");
    assert_mainless_args_rejected(&mainless_test.output().expect("mainless package test"));

    let mut mainless_entry_run = topaz();
    mainless_entry_run
        .current_dir(&dir)
        .args(["run", "src/main.tpz"])
        .args(["--language-version", "5.19"])
        .arg("--")
        .arg("--flag");
    assert_mainless_args_rejected(
        &mainless_entry_run
            .output()
            .expect("mainless selected-entry run"),
    );

    let rejected_build_dir = dir.join("rejected-build-out");
    let mut mainless_build = topaz();
    mainless_build
        .current_dir(&dir)
        .arg("build")
        .arg("--locked")
        .arg("--out-dir")
        .arg(&rejected_build_dir)
        .arg("--run")
        .arg("--")
        .arg("--flag");
    assert_mainless_args_rejected(&mainless_build.output().expect("mainless package build"));
    assert!(!rejected_build_dir.exists(), "{rejected_build_dir:?}");

    let mut mainless_self_test = topaz();
    mainless_self_test
        .current_dir(&dir)
        .arg("test")
        .arg("--locked")
        .args(["--compiler", "self"])
        .arg("--")
        .arg("--flag");
    assert_mainless_args_rejected(
        &mainless_self_test
            .output()
            .expect("mainless self package test"),
    );

    let rejected_self_build_dir = dir.join("rejected-self-build-out");
    let mut mainless_self_build = topaz();
    mainless_self_build
        .current_dir(&dir)
        .arg("build")
        .arg("--locked")
        .args(["--compiler", "self"])
        .arg("--out-dir")
        .arg(&rejected_self_build_dir)
        .arg("--run")
        .arg("--")
        .arg("--flag");
    assert_mainless_args_rejected(
        &mainless_self_build
            .output()
            .expect("mainless self package build"),
    );
    assert!(
        !rejected_self_build_dir.exists(),
        "{rejected_self_build_dir:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn package_mode_rejects_bad_explicit_main_signature_before_execution() {
    let dir = std::env::temp_dir().join("topaz_package_bad_explicit_main_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src mkdir");
    let manifest = r#"[package]
name = "pkg_mode"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[dependencies]
std = "5.4"
"#;
    std::fs::write(dir.join("topaz.toml"), manifest).expect("manifest");
    std::fs::write(dir.join("topaz.lock"), package_lock(manifest)).expect("lock");
    std::fs::write(
        dir.join("src/main.tpz"),
        r#"export function main() -> int {
  return 0
}
"#,
    )
    .expect("entry");

    for command in ["check", "run"] {
        let out = topaz()
            .arg(command)
            .arg("--root")
            .arg(&dir)
            .arg("--locked")
            .output()
            .expect("binary runs");
        assert!(!out.status.success(), "{command}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("must be non-generic and have signature"),
            "{command}: {out:?}"
        );
    }

    let out = topaz()
        .arg("build")
        .arg("--root")
        .arg(&dir)
        .arg("--locked")
        .arg("--out-dir")
        .arg(dir.join("build-out"))
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("must be non-generic and have signature"),
        "{out:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn package_mode_imports_a_locked_local_path_dependency() {
    let base = std::env::temp_dir().join("topaz_package_import_test");
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("root");
    let dep = base.join("local_schema");
    std::fs::create_dir_all(root.join("src")).expect("root mkdir");
    std::fs::create_dir_all(dep.join("src")).expect("dep mkdir");
    std::fs::write(
        dep.join("topaz.toml"),
        r#"[package]
name = "local_schema"
version = "0.1.0"
language = "5.4"
entry = "src/lib.tpz"

[exports]
module = "src/lib.tpz"
"#,
    )
    .expect("dep manifest");
    std::fs::write(
        dep.join("src/lib.tpz"),
        "export const schema = \"dep-ok\"\n",
    )
    .expect("dep source");
    let dep_hash = topaz_package::package_content_hash(&dep).expect("dep hash");
    let root_manifest = format!(
        r#"[package]
name = "root_pkg"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[dependencies]
local_schema = {{ path = "../local_schema", hash = "{dep_hash}" }}
"#
    );
    std::fs::write(root.join("topaz.toml"), &root_manifest).expect("root manifest");
    std::fs::write(
        root.join("src/main.tpz"),
        "import local_schema\nprint(local_schema.schema)\n",
    )
    .expect("root source");

    let out = topaz()
        .arg("lock")
        .arg("--root")
        .arg(&root)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let lock = std::fs::read_to_string(root.join("topaz.lock")).expect("lock written");
    assert!(lock.contains("source = \"root\""), "{lock}");
    assert!(lock.contains("path = \"../local_schema\""), "{lock}");
    assert!(lock.contains(&format!("hash = \"{dep_hash}\"")), "{lock}");

    let out = topaz()
        .arg("run")
        .arg("--root")
        .arg(&root)
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("dep-ok"),
        "{out:?}"
    );

    let out = topaz()
        .arg("emit")
        .arg("--root")
        .arg(&root)
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("pub fn run_with_host"),
        "{out:?}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn package_mode_imports_a_locked_vendored_registry_dependency() {
    let base = std::env::temp_dir().join("topaz_registry_vendor_import_test");
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("root");
    let dep = root.join("vendor/greeter/1.0.0");
    std::fs::create_dir_all(root.join("src")).expect("root mkdir");
    std::fs::create_dir_all(dep.join("src")).expect("dep mkdir");
    std::fs::write(
        dep.join("topaz.toml"),
        r#"[package]
name = "greeter"
version = "1.0.0"
language = "5.4"
entry = "src/lib.tpz"

[exports]
module = "src/lib.tpz"
"#,
    )
    .expect("dep manifest");
    std::fs::write(
        dep.join("src/lib.tpz"),
        "export const message = \"vendored-ok\"\n",
    )
    .expect("dep source");
    let dep_hash = topaz_package::package_content_hash(&dep).expect("dep hash");
    let root_manifest = r#"[package]
name = "root_pkg"
version = "0.1.0"
language = "5.4"
entry = "src/main.tpz"

[dependencies]
greeter = "1.0.0"
"#;
    std::fs::write(root.join("topaz.toml"), root_manifest).expect("root manifest");
    std::fs::write(
        root.join("src/main.tpz"),
        "import greeter\nprint(greeter.message)\n",
    )
    .expect("root source");
    let out = topaz()
        .arg("lock")
        .arg("--root")
        .arg(&root)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let lock = std::fs::read_to_string(root.join("topaz.lock")).expect("lock");
    assert!(lock.contains("source = \"registry\""), "{lock}");
    assert!(lock.contains(&format!("hash = \"{dep_hash}\"")), "{lock}");

    let out = topaz()
        .arg("run")
        .arg("--root")
        .arg(&root)
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("vendored-ok"),
        "{out:?}"
    );

    let out = topaz()
        .arg("check")
        .arg("--root")
        .arg(&root)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("requires `--locked`"),
        "{out:?}"
    );

    std::fs::write(
        dep.join("src/lib.tpz"),
        "export const message = \"changed\"\n",
    )
    .expect("mutate dep");
    let out = topaz()
        .arg("check")
        .arg("--root")
        .arg(&root)
        .arg("--locked")
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("content hash is stale"),
        "{out:?}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

fn multimodule_entry() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/main.tpz"
    ))
}

#[test]
fn emit_lowers_a_namespace_multi_module_program() {
    // CDR-006 §17: a namespace `import mathlib` lowers the imported module to a
    // record of its exports bound under the alias, so `mathlib.double(x)` is an
    // ordinary record member access. This assertion checks that emitted form;
    // the direct execution assertion checks the result separately.
    let out = rust_topaz()
        .arg("emit")
        .arg(multimodule_entry())
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let rust = String::from_utf8_lossy(&out.stdout);
    assert!(
        rust.contains("Value::record([(\"double\".to_string()"),
        "module-as-record: {rust}"
    );
    assert!(
        rust.contains("member_value_required"),
        "qualified ref: {rust}"
    );
}

#[test]
fn run_executes_a_multi_module_program() {
    // The interpreter runs the same multi-module program (the build path is proven
    // to match it manually).
    let out = rust_topaz()
        .arg("run")
        .arg(multimodule_entry())
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("double the answer is 84"),
        "{out:?}"
    );
}

#[test]
fn emit_lowers_a_module_function_reading_a_later_const() {
    // §4/§17: the interpreter's per-module const pass hoists a module's consts before
    // its functions are bound, so a function may read a `const` declared textually
    // LATER (`function getK() { K }` … `const K = 7`). The static checker hoists module
    // consts the same way, so this checks cleanly, and the emitter lowers it —
    // `run`, the default-checked path, and the compiled binary all agree ("K is 7").
    let entry = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/use_fwd_const.tpz"
    ));
    // It now passes `check` (the workaround `--unchecked` is retired).
    let checked = rust_topaz()
        .arg("check")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(checked.status.success(), "check should pass: {checked:?}");
    let emitted = rust_topaz()
        .arg("emit")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(
        emitted.status.success(),
        "emit should lower it: {emitted:?}"
    );
    let ran = rust_topaz()
        .arg("run")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    assert!(
        String::from_utf8_lossy(&ran.stdout).contains("K is 7"),
        "{ran:?}"
    );
}

#[test]
fn run_and_emit_type_check_by_default_and_unchecked_opts_out() {
    // CDR-003 §13: `run`/`emit`/`build` statically type-check by default, so a
    // type-incorrect program is rejected with the SAME diagnostic `check` reports and
    // never reaches the interpreter or the emitter. `fwd_init_fault.tpz` is a forward
    // init-order fault (`let answer = compute()` reaches a `function compute` declared
    // later): the checker rejects it (TPZ5002), exactly as the interpreter faults.
    let entry = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/fwd_init_fault.tpz"
    ));

    // run: rejected by default, with the checker's diagnostic.
    let run_default = rust_topaz()
        .arg("run")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(!run_default.status.success(), "{run_default:?}");
    assert!(
        String::from_utf8_lossy(&run_default.stderr).contains("TPZ5002"),
        "{run_default:?}"
    );

    // emit: rejected by default, same diagnostic — nothing is lowered to stdout.
    let emit_default = rust_topaz()
        .arg("emit")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(!emit_default.status.success(), "{emit_default:?}");
    assert!(emit_default.stdout.is_empty(), "{emit_default:?}");
    assert!(
        String::from_utf8_lossy(&emit_default.stderr).contains("TPZ5002"),
        "{emit_default:?}"
    );

    // --unchecked opts out: the checker is bypassed and the interpreter runs. A program
    // the checker rejects but the runtime accepts (`use_fwd_const.tpz`, before the
    // a forced `--unchecked` case) now runs on EITHER path; under `--unchecked` the
    // checker is skipped entirely and the program runs to completion.
    let runnable = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/use_fwd_const.tpz"
    ));
    let run_unchecked = rust_topaz()
        .arg("run")
        .arg("--unchecked")
        .arg(runnable)
        .output()
        .expect("binary runs");
    assert!(run_unchecked.status.success(), "{run_unchecked:?}");
}

#[test]
fn unchecked_is_rejected_for_non_execution_commands() {
    // CDR-003 §13: `--unchecked` applies to run/emit/build only. Using it anywhere
    // else (here, `check`) is an early hard error, ahead of dispatch.
    let entry = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/use_fwd_const.tpz"
    ));
    let out = topaz()
        .arg("check")
        .arg("--unchecked")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("`--unchecked` applies to run/emit/build only"),
        "{out:?}"
    );

    // With NO command at all, `--unchecked` is still a usage error (the guard runs
    // ahead of the help/no-command path), not a silent help print.
    let bare = topaz().arg("--unchecked").output().expect("binary runs");
    assert!(!bare.status.success(), "{bare:?}");
    assert!(
        String::from_utf8_lossy(&bare.stderr)
            .contains("`--unchecked` applies to run/emit/build only"),
        "{bare:?}"
    );

    // Alongside `--version`/`-V` (which carries no command verb) `--unchecked` is also
    // rejected — it must never be silently ignored (§13.2). The guard runs ahead of the
    // `--version` early-return, so this errors instead of printing the version.
    let with_version = topaz()
        .arg("--unchecked")
        .arg("--version")
        .output()
        .expect("binary runs");
    assert!(!with_version.status.success(), "{with_version:?}");
    assert!(
        String::from_utf8_lossy(&with_version.stderr)
            .contains("`--unchecked` applies to run/emit/build only"),
        "{with_version:?}"
    );
    // Sanity: `--version` on its own is unaffected.
    let version = topaz().arg("--version").output().expect("binary runs");
    assert!(version.status.success(), "{version:?}");
    assert!(
        String::from_utf8_lossy(&version.stdout).contains("Topaz"),
        "{version:?}"
    );
}

#[test]
fn check_and_run_report_the_same_diagnostic_for_an_absolute_entry() {
    // CDR-003 §13.6: a run/emit rejection is the `topaz check` diagnostic stream
    // verbatim. That requires `check` to resolve an ABSOLUTE entry the same way
    // run/emit do (via split_absolute); before that fix `check <abs>` reported
    // TPZ3001 "does not exist" while `run <abs>` reached the checker (TPZ5002).
    // `fwd_init_fault.tpz` is checker-rejected with TPZ5002 (a forward init-order
    // fault), the same diagnostic the interpreter would fault with.
    let entry = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/fwd_init_fault.tpz"
    ));
    assert!(
        entry.is_absolute(),
        "fixture path must be absolute: {entry:?}"
    );

    let checked = rust_topaz()
        .arg("check")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(!checked.status.success(), "{checked:?}");
    let check_err = String::from_utf8_lossy(&checked.stderr);
    assert!(check_err.contains("TPZ5002"), "{check_err}");

    let ran = rust_topaz()
        .arg("run")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(!ran.status.success(), "{ran:?}");
    let run_err = String::from_utf8_lossy(&ran.stderr);
    assert!(run_err.contains("TPZ5002"), "{run_err}");
}

#[test]
fn value_flags_before_the_verb_do_not_block_unchecked() {
    // Regression (CDR-003 §13): the `--unchecked` misuse guard must inspect the TRUE
    // command verb, not a value-flag (`--root`/`--out-dir`) that happens to precede it.
    // Before the guard moved after value-flag stripping, `topaz --out-dir <x> run
    // --unchecked <entry>` wrongly hit "applies to run/emit/build only". `use_fwd_const`
    // runs to completion ("K is 7"); `--unchecked` must reach it regardless.
    let entry = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/use_fwd_const.tpz"
    ));

    // `run` does not consume `--out-dir`; placing it before the verb must NOT turn a
    // valid `run --unchecked` into a misuse error.
    let run_out = topaz()
        .arg("--out-dir")
        .arg(std::env::temp_dir().join("topaz_unused_outdir"))
        .arg("run")
        .arg("--unchecked")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(run_out.status.success(), "{run_out:?}");
    assert!(
        String::from_utf8_lossy(&run_out.stdout).contains("K is 7"),
        "{run_out:?}"
    );

    // `emit` with the `--out-dir` value-flag BEFORE the verb must scaffold, not misfire.
    // (emit shares `resolve_and_lower`'s gate with build, so this covers build's path.)
    let dir = std::env::temp_dir().join("topaz_flag_order_emit_test");
    let _ = std::fs::remove_dir_all(&dir);
    let emit_out = topaz()
        .arg("--out-dir")
        .arg(&dir)
        .arg("emit")
        .arg("--unchecked")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(emit_out.status.success(), "{emit_out:?}");
    assert!(
        dir.join("Cargo.toml").exists(),
        "scaffold written: {emit_out:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emit_lowers_a_selected_import() {
    // §17: `import mathlib { double as d, ANSWER }` binds each selected export — with
    // an optional `as` alias — to the imported module's record field, so `d` and
    // `ANSWER` are entry locals lowered via member_value_required. run and the compiled
    // binary agree ("selected 84"). (run asserted here; the build path verified
    // manually.)
    let entry = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/select_main.tpz"
    ));
    let emitted = rust_topaz()
        .arg("emit")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(emitted.status.success(), "{emitted:?}");
    assert!(
        String::from_utf8_lossy(&emitted.stdout).contains("member_value_required"),
        "{emitted:?}"
    );
    let ran = rust_topaz()
        .arg("run")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    assert!(
        String::from_utf8_lossy(&ran.stdout).contains("selected 84"),
        "{ran:?}"
    );
}

#[test]
fn selected_nominal_aliases_work_in_patterns() {
    let dir = std::env::temp_dir().join(format!(
        "topaz_selected_nominal_alias_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tempdir");
    std::fs::write(
        dir.join("sel.tpz"),
        "export enum SelectedMsg { Selected(bool) }\n\
         export record SelectedUser { name: string }\n\
         export newtype SelectedId = int\n\
         export function selected() -> SelectedMsg { SelectedMsg.Selected(true) }\n\
         export function selectedUser() -> SelectedUser { SelectedUser { name: \"Ada\" } }\n\
         export function selectedId(n: int) -> SelectedId { SelectedId(n) }\n",
    )
    .expect("selected module");
    let entry = dir.join("main.tpz");
    std::fs::write(
        &entry,
        "import sel { SelectedMsg as SM, SelectedUser as SU, SelectedId as SID, selected, selectedUser, selectedId }\n\
         let msg: SM = SM.Selected(false)\n\
         let msg2: SM = selected()\n\
         let text = match msg {\n\
           case Selected(value) => \"{value}\"\n\
         }\n\
         let user: SU = selectedUser()\n\
         let name = match user {\n\
           case SU { name } => name\n\
         }\n\
         let id: SID = selectedId(41)\n\
         let n = match id {\n\
           case SID(value) => value + 1\n\
         }\n\
         print(\"{text}/{name}/{n}\")\n",
    )
    .expect("entry");

    let checked = rust_topaz()
        .arg("check")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(checked.status.success(), "{checked:?}");
    let ran = rust_topaz()
        .arg("run")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    assert!(
        String::from_utf8_lossy(&ran.stdout).contains("false/Ada/42"),
        "{ran:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn selected_nominal_aliases_are_target_specific() {
    let dir = std::env::temp_dir().join(format!(
        "topaz_selected_nominal_collision_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tempdir");
    std::fs::write(dir.join("a.tpz"), "export enum Msg { Left }\n").expect("a module");
    std::fs::write(dir.join("b.tpz"), "export enum Msg { Right }\n").expect("b module");
    let entry = dir.join("main.tpz");
    std::fs::write(
        &entry,
        "import a { Msg as A }\n\
         import b { Msg as B }\n\
         let left: A = A.Left\n\
         let right: B = B.Right\n\
         print(\"{left}/{right}\")\n",
    )
    .expect("entry");

    let checked = rust_topaz()
        .arg("check")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(checked.status.success(), "{checked:?}");
    let ran = rust_topaz()
        .arg("run")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    let stdout = String::from_utf8_lossy(&ran.stdout);
    assert!(stdout.contains("Msg.Left/Msg.Right"), "{ran:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emit_lowers_multi_segment_import_paths() {
    // §17: a DOTTED import path — `import lib.math` (namespace, bound under its LAST
    // segment `math`, so `math.triple`) and `import lib.text { shout }` (selected) —
    // keys by the module's dotted identity (`segments.join(".")`), matching the
    // resolver and interpreter. The assertions below check emission and direct execution.
    let entry = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/nested/main.tpz"
    ));
    let emitted = rust_topaz()
        .arg("emit")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(emitted.status.success(), "{emitted:?}");
    let ran = rust_topaz()
        .arg("run")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    assert!(
        String::from_utf8_lossy(&ran.stdout).contains("nested 21!"),
        "{ran:?}"
    );
}

#[test]
fn emit_lowers_a_transitive_chain() {
    // §17 TRANSITIVE: an imported module may itself `import` another — `main → tmid → tbase`.
    // Each non-entry module lowers ONCE under its canonical name `__mod_<id>` in dependency
    // order, so `tmid` (built after `tbase`) references `tbase`'s record. The
    // assertions below check emission and the `transitive 12` execution result.
    let entry = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/transitive_main.tpz"
    ));
    let emitted = rust_topaz()
        .arg("emit")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(
        emitted.status.success(),
        "should lower transitively: {emitted:?}"
    );
    let ran = rust_topaz()
        .arg("run")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    assert!(
        String::from_utf8_lossy(&ran.stdout).contains("transitive 12"),
        "{ran:?}"
    );
}

#[test]
fn emit_lowers_a_diamond_import() {
    // §17 DIAMOND: `main → (dleft, dright) → tbase`. `tbase` lowers ONCE (the canonical
    // build-once binding), shared by both importers — no duplicate init. The
    // assertions below check emission and the `diamond 21` execution result.
    let entry = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/diamond_main.tpz"
    ));
    let emitted = rust_topaz()
        .arg("emit")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(
        emitted.status.success(),
        "should lower the diamond: {emitted:?}"
    );
    let ran = rust_topaz()
        .arg("run")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    assert!(
        String::from_utf8_lossy(&ran.stdout).contains("diamond 21"),
        "{ran:?}"
    );
}

#[test]
fn emit_lowers_a_module_selected_import() {
    // §17: a SELECTED import INSIDE a non-entry module — `modsel_mid` does `import tbase { BASE }`
    // and uses `BASE`. Each selected export binds off the target's canonical record via
    // `member_value_required`, exactly as the entry's selected import. The
    // assertions below check emission and the `modsel 8` execution result.
    let entry = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/modsel_main.tpz"
    ));
    let emitted = rust_topaz()
        .arg("emit")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(
        emitted.status.success(),
        "should lower the module selected import: {emitted:?}"
    );
    let ran = rust_topaz()
        .arg("run")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(ran.status.success(), "{ran:?}");
    assert!(
        String::from_utf8_lossy(&ran.stdout).contains("modsel 8"),
        "{ran:?}"
    );
}

#[test]
fn emit_refuses_a_const_reading_an_import() {
    // §4/§17 soundness: a top-level `const` whose initializer is not a constant
    // expression — here a member access on an import, `const X = mathlib.ANSWER` —
    // is rejected by BOTH engines. The interpreter's load-time const pass faults it
    // (TPZ5001: a member expression is not a constant expression). The emitter
    // mirrors that const-expression allow-list and refuses, rather than fall back to
    // a record/prelude member read and compile a binary the interpreter rejects.
    let entry = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/const_import.tpz"
    ));
    let emitted = rust_topaz()
        .arg("emit")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(!emitted.status.success(), "emit must refuse: {emitted:?}");
    // parity: the interpreter rejects it too (its runtime const pass).
    let ran = rust_topaz()
        .arg("run")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(!ran.status.success(), "interp must reject: {ran:?}");
}

#[test]
fn emit_refuses_a_non_constant_const_in_an_imported_module() {
    // §4/§17 soundness: the interpreter runs its const pass for EVERY module, so a
    // non-constant top-level `const` in an IMPORTED module (`export const BAD =
    // toInt.field`) is rejected during that module's initialization (TPZ5001). The
    // emitter holds an imported module's top-level consts to the same rule, refusing
    // rather than lowering the namespace record with a divergent member read.
    let entry = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multimodule/use_nonconst.tpz"
    ));
    let emitted = rust_topaz()
        .arg("emit")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(!emitted.status.success(), "emit must refuse: {emitted:?}");
    let ran = rust_topaz()
        .arg("run")
        .arg(entry)
        .output()
        .expect("binary runs");
    assert!(!ran.status.success(), "interp must reject: {ran:?}");
}

#[test]
fn emit_refusal_renders_a_located_tpz6001() {
    // A WELL-FORMED program the native emitter cannot lower YET surfaces as a
    // located TPZ6001 diagnostic — the emitter owns the code, the offending span,
    // and the "still runs under `topaz run`" remedy; the CLI renders it like any
    // other diagnostic (CDR-001 §5). `--unchecked` takes the emitter path directly,
    // so the gap (a free identifier on line 3) is the emitter's, not the checker's,
    // and the caret lands on THAT line rather than the program start.
    let dir = std::env::temp_dir().join("topaz_emit_tpz6001");
    let _ = std::fs::create_dir_all(&dir);
    let entry = dir.join("free.tpz");
    std::fs::write(&entry, "let a = 1\nlet b = 2\nlet c = a + nope + b\n").expect("write fixture");
    let out = topaz()
        .arg("emit")
        .arg("--unchecked")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "emit must refuse: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error[TPZ6001]"),
        "want a TPZ6001 code:\n{stderr}"
    );
    assert!(
        stderr.contains("free.tpz:3:"),
        "want the offending line located:\n{stderr}"
    );
    assert!(stderr.contains('^'), "want a caret underline:\n{stderr}");
    assert!(
        stderr.contains("topaz run"),
        "want the remedy note:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emit_preserves_a_positional_argument_after_a_named_one_fault() {
    // §5: a positional argument may not FOLLOW a named one (valid order is
    // `positional* named*`). The checker rejects it, so it reaches the emitter only
    // via `--unchecked`. The emitter must not reorder or mis-bind the arguments; it
    // lowers the same span-carrying fault the interpreter would raise at runtime.
    let dir = std::env::temp_dir().join("topaz_emit_arg_order");
    let _ = std::fs::create_dir_all(&dir);
    let entry = dir.join("order.tpz");
    std::fs::write(
        &entry,
        "function g(a: int, b: int) -> int { a + b }\ng(a: 1, 2)\n",
    )
    .expect("write fixture");
    let out = topaz()
        .arg("emit")
        .arg("--unchecked")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(
        out.status.success(),
        "emit must preserve the fault in output: {out:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("positional arguments may not follow named arguments"),
        "want the source-order fault in emitted Rust:\n{stdout}"
    );
    assert!(
        stdout.contains("Span::new(FileId(0), 44, 54)"),
        "want the offending call span in emitted Rust:\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emit_redeclaration_refusal_carets_the_offending_line() {
    // A STATEMENT-structural refusal the native emitter raises — a same-scope
    // redeclaration — must caret the OFFENDING declaration, not the line-1
    // whole-program fallback (CDR-001 §5 TPZ6001). A `for`-loop variable shares
    // the body's scope, so re-binding its name in the body is a same-scope
    // redeclaration the resolver permits but the emitter refuses (the resolver's
    // top-level redeclaration check, TPZ3008, does not reach a loop-body shadow),
    // so this genuinely exercises the emitter's refusal. The offending `let x` is
    // on line 2; the caret must land there, not on line 1.
    let dir = std::env::temp_dir().join("topaz_emit_redecl_tpz6001");
    let _ = std::fs::create_dir_all(&dir);
    let entry = dir.join("redecl.tpz");
    std::fs::write(&entry, "for x in [1, 2] {\n  let x = 5\n}").expect("write fixture");
    let out = topaz()
        .arg("emit")
        .arg("--unchecked")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "emit must refuse: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error[TPZ6001]"),
        "want a TPZ6001 code:\n{stderr}"
    );
    // The offending redeclaration is on line 2 — the caret must be there, NOT the
    // misleading line-1 whole-program fallback the coarse span rendered.
    assert!(
        stderr.contains("redecl.tpz:2:"),
        "want the offending line (2) located, not line 1:\n{stderr}"
    );
    assert!(
        !stderr.contains("redecl.tpz:1:"),
        "must NOT caret line 1:\n{stderr}"
    );
    assert!(stderr.contains('^'), "want a caret underline:\n{stderr}");
    assert!(
        stderr.contains("topaz run"),
        "want the remedy note:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emit_break_outside_loop_refusal_carets_the_offending_line() {
    // A CONTROL-FLOW structural refusal — a `break` reached outside any loop — is
    // a static error the native emitter raises (the interpreter would runtime-fault
    // it). It must caret the offending `break` statement, not the line-1
    // whole-program fallback (CDR-001 §5 TPZ6001). With a leading binding the
    // `break` is on line 2, so the caret must land there.
    let dir = std::env::temp_dir().join("topaz_emit_break_tpz6001");
    let _ = std::fs::create_dir_all(&dir);
    let entry = dir.join("brk.tpz");
    std::fs::write(&entry, "let a = 1\nbreak").expect("write fixture");
    let out = topaz()
        .arg("emit")
        .arg("--unchecked")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "emit must refuse: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error[TPZ6001]"),
        "want a TPZ6001 code:\n{stderr}"
    );
    // The offending `break` is on line 2 — caret there, NOT the line-1 fallback.
    assert!(
        stderr.contains("brk.tpz:2:"),
        "want the offending line (2) located, not line 1:\n{stderr}"
    );
    assert!(
        !stderr.contains("brk.tpz:1:"),
        "must NOT caret line 1:\n{stderr}"
    );
    assert!(stderr.contains('^'), "want a caret underline:\n{stderr}");
    assert!(
        stderr.contains("topaz run"),
        "want the remedy note:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emit_match_list_element_refusal_carets_the_case_pattern() {
    // An unsupported pattern shape NESTED in a `case` pattern (here an unknown
    // constructor subpattern in a list element, `[Foo(x), y]`) must caret the OFFENDING
    // element on the case line — NOT the whole `match` on an earlier line. The inner
    // `emit_subpattern` refusal must carry its own element span so the enclosing
    // `emit_expr` wrapper around the `match` cannot coarsen it (CDR-001 §5 TPZ6001).
    let dir = std::env::temp_dir().join("topaz_emit_match_elem_tpz6001");
    let _ = std::fs::create_dir_all(&dir);
    let entry = dir.join("melem.tpz");
    // `match` is on line 2; the offending `[Foo(x), y]` case is on line 3.
    std::fs::write(
        &entry,
        "let xs = [1, 2]\nmatch xs {\n  case [Foo(x), y] => y\n  case _ => 0\n}\n",
    )
    .expect("write fixture");
    let out = topaz()
        .arg("emit")
        .arg("--unchecked")
        .arg(&entry)
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "emit must refuse: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error[TPZ6001]"),
        "want a TPZ6001 code:\n{stderr}"
    );
    // The caret must land on the case pattern (line 3), NOT the `match` (line 2).
    assert!(
        stderr.contains("melem.tpz:3:"),
        "want the offending case pattern (line 3) located, not the match line:\n{stderr}"
    );
    assert!(
        !stderr.contains("melem.tpz:2:"),
        "must NOT caret the whole `match` on line 2:\n{stderr}"
    );
    assert!(stderr.contains('^'), "want a caret underline:\n{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_fails_at_resolution_before_cargo() {
    // A nonexistent entry fails at resolution, before scaffolding or cargo, so
    // this failure path cannot print a "built" success line.
    let dir = std::env::temp_dir().join("topaz_build_noexist_test");
    let _ = std::fs::remove_dir_all(&dir);
    let out = topaz()
        .arg("build")
        .arg(repo_root().join("does/not/exist.tpz"))
        .arg("--out-dir")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(!out.status.success());
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("built `"),
        "{out:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
