use super::support::*;

#[test]
fn compiler_status_reports_validated_dual_engine_contract() {
    let out = topaz()
        .args(["compiler", "status", "--json"])
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(out.stderr.is_empty(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("{\"schema\":\"topaz.compiler-support/v2\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"defaultCompiler\":\"rust\""), "{stdout}");
    assert!(stdout.contains("\"recoveryCompiler\":\"rust\""), "{stdout}");
    assert!(
        stdout.contains("\"compatibilityCompiler\":\"rust\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"selfCompilationProductSchema\":\"topaz.self-compilation-product/v1\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"silentFallback\":false"), "{stdout}");
    assert!(
        stdout.contains("\"selector\":\"self\",\"producer\":\"topaz-stage2\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"programImageSha256\":\"sha256:"),
        "{stdout}"
    );
    assert!(
        stdout.contains(
            "\"command\":\"check <entry>\",\"rust\":\"supported\",\"self\":\"supported\",\"omitted\":\"rust\""
        ),
        "{stdout}"
    );
    assert_eq!(stdout.matches("\"omitted\":\"self\"").count(), 0);
    assert_eq!(stdout.matches("\"omitted\":\"rust\"").count(), 26);
}

#[test]
fn default_rust_matches_explicit_rust_and_keeps_self_independent() {
    let dir = std::env::temp_dir().join(format!("topaz_cli_self_check_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create tempdir");
    let entry = dir.join("main.tpz");
    std::fs::write(
        &entry,
        "let answer: int = 42\nfunction main() -> int { 0 }\n",
    )
    .expect("write entry");

    let omitted_rust = topaz()
        .arg("check")
        .arg(&entry)
        .output()
        .expect("binary runs");
    let explicit_self = topaz()
        .arg("check")
        .arg(&entry)
        .args(["--compiler", "self"])
        .output()
        .expect("binary runs");
    let explicit_rust = topaz()
        .arg("check")
        .arg(&entry)
        .args(["--compiler", "rust"])
        .output()
        .expect("binary runs");
    assert!(explicit_rust.status.success(), "{explicit_rust:?}");
    assert_eq!(omitted_rust.status, explicit_rust.status);
    assert_eq!(omitted_rust.stdout, explicit_rust.stdout);
    assert_eq!(omitted_rust.stderr, explicit_rust.stderr);
    assert!(explicit_self.status.success(), "{explicit_self:?}");
    let stdout = String::from_utf8_lossy(&omitted_rust.stdout);
    assert!(stdout.contains("types-ok (1 module)"), "{stdout}");
    assert!(omitted_rust.stderr.is_empty(), "{omitted_rust:?}");

    let omitted_run = topaz()
        .arg("run")
        .arg(&entry)
        .output()
        .expect("binary runs");
    let explicit_rust_run = topaz()
        .arg("run")
        .arg(&entry)
        .args(["--compiler", "rust"])
        .output()
        .expect("binary runs");
    assert_eq!(omitted_run.status, explicit_rust_run.status);
    assert_eq!(omitted_run.stdout, explicit_rust_run.stdout);
    assert_eq!(omitted_run.stderr, explicit_rust_run.stderr);

    let missing = dir.join("missing.tpz");
    let declined = topaz()
        .args(["refactor", "organize-imports"])
        .arg(&missing)
        .args(["--compiler", "self"])
        .output()
        .expect("binary runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!declined.status.success(), "{declined:?}");
    assert!(declined.stdout.is_empty(), "{declined:?}");
    let stderr = String::from_utf8_lossy(&declined.stderr);
    assert!(stderr.contains("does not support"), "{stderr}");
    assert!(stderr.contains("--compiler rust"), "{stderr}");
    assert!(stderr.contains("not executed"), "{stderr}");
    assert!(!stderr.contains("cannot read"), "{stderr}");
    assert!(!stderr.contains("No such file"), "{stderr}");

    let old_mode = topaz()
        .arg("check")
        .arg(&missing)
        .args(["--compiler", "self", "--language-version", "5.13"])
        .output()
        .expect("binary runs");
    assert!(!old_mode.status.success(), "{old_mode:?}");
    assert!(old_mode.stdout.is_empty(), "{old_mode:?}");
    let stderr = String::from_utf8_lossy(&old_mode.stderr);
    assert!(
        stderr.contains("exact language profile admitted"),
        "{stderr}"
    );
    assert!(stderr.contains("--compiler rust"), "{stderr}");
    assert!(!stderr.contains("cannot read"), "{stderr}");
    assert!(!stderr.contains("No such file"), "{stderr}");
}

#[test]
fn v516_explicit_self_executes_stage2_check_and_formatter_routes() {
    let root =
        std::env::temp_dir().join(format!("topaz_cli_v516_self_routes_{}", std::process::id()));
    let entry = root.join("main.tpz");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create tempdir");
    std::fs::write(&entry, "function main() -> int { 0 }\n").expect("write entry");

    let check = topaz()
        .arg("check")
        .arg(&entry)
        .args(["--language-version", "5.16", "--compiler", "self"])
        .env("TOPAZ_SELF_FRONTEND_METRICS", "1")
        .output()
        .expect("v5.16 check runs");
    assert!(check.status.success(), "{check:?}");
    let check_stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        check_stderr.contains("topaz-self-frontend-route: compilation-product"),
        "{check_stderr}"
    );

    let format = topaz()
        .arg("fmt")
        .arg("--check")
        .arg(&entry)
        .args(["--language-version", "5.16", "--compiler", "self"])
        .env("TOPAZ_SELF_FRONTEND_METRICS", "1")
        .output()
        .expect("v5.16 formatter runs");
    assert!(format.status.success(), "{format:?}");
    let format_stderr = String::from_utf8_lossy(&format.stderr);
    assert!(
        format_stderr.contains("topaz-self-frontend-route: formatter-parse"),
        "{format_stderr}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

fn dual_output(arguments: &[&str], compiler: &str) -> std::process::Output {
    topaz()
        .args(arguments)
        .args(["--compiler", compiler])
        .output()
        .expect("binary runs")
}

#[cfg(target_os = "linux")]
fn assert_current_file_and_package_checks(root: &Path, package: &Path) {
    for compiler in ["rust", "self"] {
        let file_check = topaz()
            .arg("check")
            .arg("--root")
            .arg(root)
            .args(["--language-version", "5.19"])
            .args(["--compiler", compiler])
            .arg(root.join("main.tpz"))
            .output()
            .expect("compiler check runs");
        assert!(file_check.status.success(), "{compiler}: {file_check:?}");

        let package_check = topaz()
            .arg("check")
            .arg("--root")
            .arg(package)
            .arg("--locked")
            .args(["--compiler", compiler])
            .output()
            .expect("package compiler check runs");
        assert!(
            package_check.status.success(),
            "{compiler}: {package_check:?}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn non_unicode_directory_entry_does_not_block_a_valid_module_import() {
    use std::os::unix::ffi::OsStringExt;

    let root = std::env::temp_dir().join(format!(
        "topaz_cli_non_unicode_directory_entry_{}",
        std::process::id()
    ));
    let package = root.join("package");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create temp directory");
    std::fs::write(
        root.join("main.tpz"),
        "import lib { value }\nlet answer = value\n",
    )
    .expect("write entry module");
    std::fs::write(root.join("lib.tpz"), "export let value = 42\n").expect("write imported module");
    std::fs::write(
        root.join(std::ffi::OsString::from_vec(vec![b'u', b'n', 0xff])),
        b"not a Topaz module",
    )
    .expect("write non-Unicode directory entry");
    std::fs::create_dir_all(package.join("src")).expect("create package source directory");
    let manifest = package_manifest().replace("language = \"5.4\"", "language = \"5.19\"");
    std::fs::write(package.join("topaz.toml"), &manifest).expect("write package manifest");
    std::fs::write(package.join("topaz.lock"), package_lock(&manifest))
        .expect("write package lock");
    std::fs::write(
        package.join("src/main.tpz"),
        "import src.lib { value }\nlet answer = value\n",
    )
    .expect("write package entry module");
    std::fs::write(package.join("src/lib.tpz"), "export let value = 42\n")
        .expect("write imported package module");
    std::fs::write(
        package
            .join("src")
            .join(std::ffi::OsString::from_vec(vec![b'u', b'n', 0xff])),
        b"not a Topaz module",
    )
    .expect("write non-Unicode package directory entry");

    assert_current_file_and_package_checks(&root, &package);

    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(target_os = "linux")]
#[test]
fn distinct_non_unicode_physical_paths_do_not_alias() {
    use std::os::unix::ffi::OsStringExt;

    let root = std::env::temp_dir().join(format!(
        "topaz_cli_distinct_non_unicode_physical_paths_{}",
        std::process::id()
    ));
    let package = root.join("package");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create temp directory");

    let first_physical = root.join(std::ffi::OsString::from_vec(vec![b'd', 0xfe]));
    let second_physical = root.join(std::ffi::OsString::from_vec(vec![b'd', 0xff]));
    std::fs::create_dir_all(&first_physical).expect("create first physical directory");
    std::fs::create_dir_all(&second_physical).expect("create second physical directory");
    std::fs::write(
        first_physical.join("lib.tpz"),
        "export let firstValue = 20\n",
    )
    .expect("write first physical module");
    std::fs::write(
        second_physical.join("lib.tpz"),
        "export let secondValue = 22\n",
    )
    .expect("write second physical module");
    std::os::unix::fs::symlink(first_physical.join("lib.tpz"), root.join("first.tpz"))
        .expect("link first physical module");
    std::os::unix::fs::symlink(second_physical.join("lib.tpz"), root.join("second.tpz"))
        .expect("link second physical module");
    std::fs::write(
        root.join("main.tpz"),
        "import first { firstValue }\nimport second { secondValue }\nlet answer = firstValue + secondValue\n",
    )
    .expect("write entry module");

    let package_source = package.join("src");
    let first_package_physical =
        package_source.join(std::ffi::OsString::from_vec(vec![b'd', 0xfe]));
    let second_package_physical =
        package_source.join(std::ffi::OsString::from_vec(vec![b'd', 0xff]));
    std::fs::create_dir_all(&first_package_physical)
        .expect("create first package physical directory");
    std::fs::create_dir_all(&second_package_physical)
        .expect("create second package physical directory");
    std::fs::write(
        first_package_physical.join("lib.tpz"),
        "export let firstValue = 20\n",
    )
    .expect("write first package physical module");
    std::fs::write(
        second_package_physical.join("lib.tpz"),
        "export let secondValue = 22\n",
    )
    .expect("write second package physical module");
    std::os::unix::fs::symlink(
        first_package_physical.join("lib.tpz"),
        package_source.join("first.tpz"),
    )
    .expect("link first package physical module");
    std::os::unix::fs::symlink(
        second_package_physical.join("lib.tpz"),
        package_source.join("second.tpz"),
    )
    .expect("link second package physical module");
    let manifest = package_manifest().replace("language = \"5.4\"", "language = \"5.19\"");
    std::fs::write(package.join("topaz.toml"), &manifest).expect("write package manifest");
    std::fs::write(package.join("topaz.lock"), package_lock(&manifest))
        .expect("write package lock");
    std::fs::write(
        package_source.join("main.tpz"),
        "import src.first { firstValue }\nimport src.second { secondValue }\nlet answer = firstValue + secondValue\n",
    )
    .expect("write package entry module");

    assert_current_file_and_package_checks(&root, &package);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dual_routes_agree_on_bounded_entry_package_and_failure() {
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/self-hosting");
    let entry = fixture_root.join("dual-toolchain-simple.tpz");
    let package = fixture_root.join("dual-toolchain-package");
    let failure = fixture_root.join("stage2-diagnostic.tpz");
    let entry = entry.to_str().expect("fixture path");
    let package = package.to_str().expect("package path");
    let failure = failure.to_str().expect("failure path");

    for arguments in [
        vec!["parse", entry],
        vec!["check", entry],
        vec!["check", entry, "--profile", "agent-pack"],
        vec!["check", entry, "--exports-json"],
        vec!["run", entry],
        vec!["test", entry],
        vec!["check", failure],
        vec!["check", failure, "--json"],
        vec!["check", "--root", package, "--locked"],
        vec!["run", "--root", package, "--locked"],
        vec!["test", "--root", package, "--locked"],
        vec!["test", "src/main.tpz", "--root", package, "--locked"],
    ] {
        let rust = dual_output(&arguments, "rust");
        let self_hosted = dual_output(&arguments, "self");
        assert_eq!(
            rust.status, self_hosted.status,
            "status drift for {arguments:?}: rust={rust:?}, self={self_hosted:?}"
        );
        assert_eq!(
            rust.stdout, self_hosted.stdout,
            "stdout drift for {arguments:?}"
        );
        assert_eq!(
            rust.stderr, self_hosted.stderr,
            "stderr drift for {arguments:?}"
        );
    }

    let rust_ast = dual_output(&["dump-ast", entry], "rust");
    let self_ast = dual_output(&["dump-ast", entry], "self");
    assert!(rust_ast.status.success(), "{rust_ast:?}");
    assert!(self_ast.status.success(), "{self_ast:?}");
    assert!(rust_ast.stderr.is_empty(), "{rust_ast:?}");
    assert!(self_ast.stderr.is_empty(), "{self_ast:?}");
    assert!(
        String::from_utf8_lossy(&rust_ast.stdout).starts_with("Program {"),
        "{rust_ast:?}"
    );
    assert!(
        String::from_utf8_lossy(&self_ast.stdout).starts_with("[\n    CanonicalPreviewAstNode {"),
        "{self_ast:?}"
    );

    let self_bench = dual_output(&["bench", entry, "--json"], "self");
    assert!(self_bench.status.success(), "{self_bench:?}");
    let bench = String::from_utf8_lossy(&self_bench.stdout);
    assert!(bench.starts_with("{\"status\":\"check-ok\""), "{bench}");
    assert!(bench.contains("\"modules\":1"), "{bench}");

    let observation = std::env::temp_dir().join(format!(
        "topaz_cli_dual_route_observation_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&observation);
    let observed = topaz()
        .args(["compiler", "observe", entry, "--terminal", "rust-source"])
        .arg("--out-dir")
        .arg(&observation)
        .args(["--compiler", "self"])
        .output()
        .expect("binary runs");
    assert!(observed.status.success(), "{observed:?}");
    assert!(observed.stderr.is_empty(), "{observed:?}");
    let validated = topaz()
        .args(["compiler", "validate"])
        .arg(&observation)
        .output()
        .expect("binary runs");
    let _ = std::fs::remove_dir_all(&observation);
    assert!(validated.status.success(), "{validated:?}");
    assert!(validated.stderr.is_empty(), "{validated:?}");
}

#[test]
fn parse_reports_parse_ok_for_a_corpus_file() {
    let file = repo_root().join("corpus/v5.1/examples/002.tpz");
    let out = topaz()
        .arg("parse")
        .arg(&file)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("parse-ok"));
}

#[test]
fn parse_renders_diagnostics_and_fails_for_invalid_input() {
    let file = repo_root().join("corpus/v5.1/invalid/tpz2002-unknown-template-tag.tpz");
    let out = topaz()
        .arg("parse")
        .arg(&file)
        .output()
        .expect("binary runs");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("TPZ2002"), "{stderr}");
}

#[test]
fn explain_reports_human_and_json_diagnostic_help() {
    let out = topaz()
        .arg("explain")
        .arg("TPZ5001")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("TPZ5001: Type mismatch"), "{stdout}");
    assert!(stdout.contains("phase: check/runtime"), "{stdout}");
    assert!(stdout.contains("fix-it:"), "{stdout}");

    let out = topaz()
        .arg("explain")
        .arg("TPZ5001")
        .arg("--json")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("\"code\":\"TPZ5001\""), "{stdout}");
    assert!(stdout.contains("\"title\":\"Type mismatch\""), "{stdout}");
    assert!(stdout.contains("\"phase\":\"check/runtime\""), "{stdout}");

    let out = topaz()
        .arg("explain")
        .arg("5001")
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("TPZ####"), "{stderr}");
}

#[test]
fn dump_ast_prints_a_program() {
    let file = repo_root().join("corpus/v5.1/examples/002.tpz");
    let out = rust_topaz()
        .arg("dump-ast")
        .arg(&file)
        .output()
        .expect("binary runs");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Program"), "{stdout}");
}

#[test]
fn check_corpus_runs_all_gates_green() {
    let out = topaz().arg("check-corpus").output().expect("binary runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}\n{:?}", out.stderr);
    assert!(stdout.contains("610 parse-ok"), "{stdout}");
    assert!(stdout.contains("v5.4:"), "{stdout}");
    assert!(stdout.contains("all gates green"), "{stdout}");
}

#[test]
fn unknown_command_fails_with_usage() {
    let out = topaz().arg("frobnicate").output().expect("binary runs");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("USAGE"));
}

#[test]
fn help_lists_product_targets() {
    let out = topaz().arg("help").output().expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("--target native|web|web-worker|web-app|http-service|python"),
        "{stdout}"
    );
    assert!(!stdout.contains("--experimental"), "{stdout}");
    assert!(
        stdout.contains("Python deployment source bundle"),
        "{stdout}"
    );
}

#[test]
fn binary_exposes_embedded_license_and_notice() {
    let license = topaz().arg("license").output().expect("binary runs");
    assert!(license.status.success(), "{license:?}");
    assert_eq!(
        license.stdout,
        include_bytes!("../../../../licenses/APACHE-2.0-PUBLIC-ARTIFACTS.txt")
    );

    let notice = topaz().arg("notice").output().expect("binary runs");
    assert!(notice.status.success(), "{notice:?}");
    assert_eq!(notice.stdout, include_bytes!("../../../../NOTICE"));
}

#[test]
fn v520_is_the_current_product_identity_and_older_lines_remain_selectable() {
    let product_version = env!("CARGO_PKG_VERSION");
    let short = topaz().arg("--version").output().expect("binary runs");
    assert!(short.status.success(), "{short:?}");
    assert_eq!(
        String::from_utf8_lossy(&short.stdout),
        format!("Topaz {product_version}\n")
    );

    let out = topaz()
        .arg("--version")
        .arg("--verbose")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(&format!("Topaz compiler {product_version}")),
        "{stdout}"
    );
    assert!(stdout.contains("Language mode: topaz-5.20"), "{stdout}");
    assert!(
        stdout.contains(&format!("Runtime: topaz_rt {product_version}")),
        "{stdout}"
    );

    let file = repo_root().join("../examples/readiness/lispex-recursive-values.tpz");
    for version in [
        "5.5", "5.6", "5.7", "5.8", "5.9", "5.10", "5.11", "5.12", "5.13", "5.14", "5.15", "5.16",
        "5.17", "5.18", "5.19",
    ] {
        let out = topaz()
            .arg("check")
            .arg(&file)
            .arg("--language-version")
            .arg(version)
            .output()
            .expect("binary runs");
        assert!(out.status.success(), "mode {version}: {out:?}");
    }
}

#[test]
fn experimental_flag_is_python_build_only() {
    let out = rust_topaz()
        .arg("build")
        .arg("--experimental")
        .arg(emit_fixture())
        .arg("--out-dir")
        .arg(std::env::temp_dir().join("topaz_experimental_scope_reject_test"))
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("legacy `--experimental` applies only to `build --target python`"),
        "{out:?}"
    );
}

#[test]
fn wrong_arity_fails_with_usage() {
    // Extra positional arguments are misuse, not noise.
    let file = repo_root().join("corpus/v5.1/examples/002.tpz");
    let out = rust_topaz()
        .arg("parse")
        .arg(&file)
        .arg("extra")
        .output()
        .expect("binary runs");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("wrong arguments"));

    let out = rust_topaz()
        .arg("check-corpus")
        .arg("extra")
        .output()
        .expect("binary runs");
    assert!(!out.status.success());

    let out = rust_topaz().arg("parse").output().expect("binary runs");
    assert!(!out.status.success());
}

#[test]
fn emit_requires_v5_2() {
    // v5.1 has no module system, so `emit` rejects it (like `check`).
    let out = topaz()
        .arg("emit")
        .arg(emit_fixture())
        .arg("--language-version")
        .arg("5.1")
        .output()
        .expect("binary runs");
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("needs v5.2"),
        "{out:?}"
    );
}

#[test]
fn emit_reports_resolution_diagnostics() {
    // A nonexistent entry surfaces the resolver's diagnostics and fails,
    // rather than emitting anything.
    let out = topaz()
        .arg("emit")
        .arg(repo_root().join("does/not/exist.tpz"))
        .output()
        .expect("binary runs");
    assert!(!out.status.success());
    assert!(out.stdout.is_empty(), "{out:?}");
}

#[test]
fn relative_entry_outside_root_is_a_plain_error_not_a_fabricated_frame() {
    // A relative entry outside `--root` must fail as a plain CLI error,
    // exactly like an absolute one — NOT as a resolver diagnostic that loads the
    // entry source only to anchor a misleading caret on byte 0.
    let dir = std::env::temp_dir().join("topaz_root_containment_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("root_dir")).expect("mkdir");
    std::fs::write(dir.join("outside.tpz"), "let x = 5\nx\n").expect("write fixture");
    let out = topaz()
        .current_dir(&dir)
        .arg("run")
        .arg("--root")
        .arg("root_dir")
        .arg("outside.tpz")
        .output()
        .expect("binary runs");
    assert!(!out.status.success(), "must refuse: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.starts_with("topaz:") && stderr.contains("is not under --root"),
        "want a plain `topaz:` containment error: {stderr}"
    );
    // No fabricated source frame: no caret, no `-->` location, no TPZ3002 code.
    assert!(
        !stderr.contains("-->") && !stderr.contains('^') && !stderr.contains("TPZ3002"),
        "must not fabricate a source frame: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn entry_file_cannot_also_be_the_source_root() {
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/self-hosting");
    let entry = "dual-toolchain-simple.tpz";
    for compiler in ["rust", "self"] {
        let out = topaz()
            .current_dir(&fixture_root)
            .args(["check", entry, "--root", entry, "--compiler", compiler])
            .output()
            .expect("binary runs");
        assert!(!out.status.success(), "{compiler} must reject: {out:?}");
        assert!(out.stdout.is_empty(), "{compiler} wrote stdout: {out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            stderr,
            "topaz: the source root `dual-toolchain-simple.tpz` must be a directory containing the entry, not the entry file itself\n",
        );
    }
}

#[test]
fn absolute_entry_preserves_the_explicit_source_root() {
    let root = std::fs::canonicalize(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/self-hosting/dual-toolchain-package"),
    )
    .expect("canonical fixture root");
    let entry = root.join("src/main.tpz");
    let mut rust_exports = None;

    for compiler in ["rust", "self"] {
        let out = topaz()
            .arg("check")
            .arg(&entry)
            .arg("--root")
            .arg(&root)
            .args(["--compiler", compiler, "--exports-json"])
            .output()
            .expect("binary runs");
        assert!(out.status.success(), "{compiler} must resolve: {out:?}");
        assert!(out.stderr.is_empty(), "{compiler} wrote stderr: {out:?}");
        let exports: JsonValue =
            serde_json::from_slice(&out.stdout).expect("exports JSON is valid");
        let identities: Vec<&str> = exports["modules"]
            .as_array()
            .expect("exports modules array")
            .iter()
            .map(|module| module["identity"].as_str().expect("module identity"))
            .collect();
        assert_eq!(identities, ["src.answer", "src.main"], "{compiler}");
        if let Some(rust_exports) = &rust_exports {
            assert_eq!(&out.stdout, rust_exports, "{compiler} export surface drift");
        } else {
            rust_exports = Some(out.stdout);
        }
    }
}

#[test]
fn compiler_observe_and_validate_are_canonical_installed_boundaries() {
    let root = std::env::temp_dir().join(format!("토파즈_compiler_observe_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(
        root.join("topaz.toml"),
        r#"[package]
name = "compiler_observe"
version = "0.1.0"
language = "5.16"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.16"
"#,
    )
    .expect("manifest");
    std::fs::write(
        root.join("src/main.tpz"),
        "import 도구 { value }\nprint(\"{value}\")\n",
    )
    .expect("entry");
    std::fs::write(root.join("도구.tpz"), "export const value = \"ok\"\n").expect("module");

    let inside = root.join("compiler-observation");
    let refused_inside = rust_topaz()
        .args(["compiler", "observe", "--out-dir"])
        .arg(&inside)
        .current_dir(&root)
        .output()
        .expect("binary runs");
    assert!(!refused_inside.status.success(), "{refused_inside:?}");
    assert!(!inside.exists());

    let output_root = std::env::temp_dir().join(format!(
        "topaz_compiler_observe_output_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&output_root);
    std::fs::create_dir_all(&output_root).expect("output root");
    let standalone_root = std::env::temp_dir().join(format!(
        "topaz_compiler_observe_standalone_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&standalone_root);
    std::fs::create_dir_all(&standalone_root).expect("standalone root");
    let standalone_entry = standalone_root.join("main.tpz");
    std::fs::write(&standalone_entry, "let answer: int = 42\n")
        .expect("standalone inherited-profile entry");
    let standalone = output_root.join("standalone-inherited-profile");
    let standalone_out = rust_topaz()
        .args(["compiler", "observe"])
        .arg(&standalone_entry)
        .args(["--language-version", "5.16", "--out-dir"])
        .arg(&standalone)
        .output()
        .expect("binary runs");
    assert!(standalone_out.status.success(), "{standalone_out:?}");

    let first = output_root.join("first");
    let out = rust_topaz()
        .args(["compiler", "observe", "--out-dir"])
        .arg(&first)
        .current_dir(&root)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let validate = topaz()
        .args(["compiler", "validate"])
        .arg(&first)
        .output()
        .expect("binary runs");
    assert!(validate.status.success(), "{validate:?}");
    let first_manifest = std::fs::read(first.join("topaz-observation.json")).expect("manifest");
    let repeat = rust_topaz()
        .args(["compiler", "observe", "--out-dir"])
        .arg(&first)
        .current_dir(&root)
        .output()
        .expect("binary runs");
    assert!(repeat.status.success(), "{repeat:?}");
    assert_eq!(
        std::fs::read(first.join("topaz-observation.json")).expect("repeated manifest"),
        first_manifest
    );

    let second_root = std::env::temp_dir().join(format!(
        "Топаз_compiler_observe_relocated_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&second_root);
    std::fs::create_dir_all(second_root.join("src")).expect("mkdir");
    for relative in ["topaz.toml", "src/main.tpz", "도구.tpz"] {
        std::fs::copy(root.join(relative), second_root.join(relative)).expect("copy");
    }
    let second = output_root.join("second");
    let out = rust_topaz()
        .args(["compiler", "observe", "--out-dir"])
        .arg(&second)
        .current_dir(&second_root)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");

    for relative in [
        "topaz-observation.json",
        "request.json",
        "response.json",
        "provenance.json",
        "source-set.jsonl",
        "tokens.jsonl",
        "ast.jsonl",
        "resolved.jsonl",
        "typed.jsonl",
        "lowered.jsonl",
        "rust-source.jsonl",
        "diagnostics.jsonl",
        "sources/000000.tpz",
        "sources/000001.tpz",
    ] {
        assert_eq!(
            std::fs::read(first.join(relative)).expect("first member"),
            std::fs::read(second.join(relative)).expect("second member"),
            "{relative} changed after relocation"
        );
    }

    for layer in ["semantic", "generated-source", "provenance"] {
        let compared = topaz()
            .args(["compiler", "compare", "--layer", layer])
            .arg(&first)
            .arg(&second)
            .output()
            .expect("binary runs");
        assert!(compared.status.success(), "{layer}: {compared:?}");
        let stdout = String::from_utf8_lossy(&compared.stdout);
        assert!(stdout.contains("\"equal\":true"), "{layer}: {stdout}");
        assert!(
            stdout.contains(&format!("\"layer\":\"{layer}\"")),
            "{layer}: {stdout}"
        );
    }
    let left_binary = output_root.join("left.bin");
    let right_binary = output_root.join("right.bin");
    std::fs::write(&left_binary, b"same binary").expect("left binary");
    std::fs::write(&right_binary, b"same binary").expect("right binary");
    let native_equal = topaz()
        .args(["compiler", "compare", "--layer", "native-binary"])
        .arg(&left_binary)
        .arg(&right_binary)
        .output()
        .expect("binary runs");
    assert!(native_equal.status.success(), "{native_equal:?}");
    std::fs::write(&right_binary, b"different binary").expect("mutate right binary");
    let native_mismatch = topaz()
        .args(["compiler", "compare", "--layer", "native-binary"])
        .arg(&left_binary)
        .arg(&right_binary)
        .output()
        .expect("binary runs");
    assert!(!native_mismatch.status.success(), "{native_mismatch:?}");
    assert!(
        String::from_utf8_lossy(&native_mismatch.stdout)
            .contains("\"firstFailingPhase\":\"native-binary\""),
        "{native_mismatch:?}"
    );

    let tokens = first.join("tokens.jsonl");
    let mut bytes = std::fs::read(&tokens).expect("tokens");
    bytes[0] ^= 1;
    std::fs::write(&tokens, &bytes).expect("mutate");
    let invalid = topaz()
        .args(["compiler", "validate"])
        .arg(&first)
        .output()
        .expect("binary runs");
    assert!(!invalid.status.success(), "{invalid:?}");
    let refused_replace = rust_topaz()
        .args(["compiler", "observe", "--out-dir"])
        .arg(&first)
        .current_dir(&root)
        .output()
        .expect("binary runs");
    assert!(!refused_replace.status.success(), "{refused_replace:?}");
    assert_eq!(
        std::fs::read(&tokens).expect("still-mutated tokens"),
        bytes,
        "an invalid managed destination must not be replaced"
    );

    std::fs::write(
        root.join("src/main.tpz"),
        "import 도구 { value }\nlet broken =\n",
    )
    .expect("rejected entry");
    let rejected = output_root.join("rejected");
    let out = rust_topaz()
        .args(["compiler", "observe", "--out-dir"])
        .arg(&rejected)
        .current_dir(&root)
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    assert!(
        !std::fs::read(rejected.join("diagnostics.jsonl"))
            .expect("diagnostics")
            .is_empty()
    );
    let validate = topaz()
        .args(["compiler", "validate"])
        .arg(&rejected)
        .output()
        .expect("binary runs");
    assert!(validate.status.success(), "{validate:?}");
    let mismatch = topaz()
        .args(["compiler", "compare"])
        .arg(&second)
        .arg(&rejected)
        .output()
        .expect("binary runs");
    assert!(!mismatch.status.success(), "{mismatch:?}");
    let mismatch_stdout = String::from_utf8_lossy(&mismatch.stdout);
    assert!(
        mismatch_stdout.contains("\"firstFailingPhase\":\"source-set\""),
        "{mismatch_stdout}"
    );

    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(second_root);
    let _ = std::fs::remove_dir_all(standalone_root);
    let _ = std::fs::remove_dir_all(output_root);
}

#[cfg(target_os = "linux")]
#[test]
fn compiler_observation_rejects_non_unicode_member_paths() {
    use std::os::unix::ffi::OsStringExt;

    let root = std::env::temp_dir().join(format!(
        "topaz_non_unicode_observation_member_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("observation root");
    let invalid = std::ffi::OsString::from_vec(b"member-\xff.json".to_vec());
    std::fs::write(root.join(invalid), b"not an admitted observation member")
        .expect("invalid-byte observation member");

    let output = topaz()
        .args(["compiler", "validate"])
        .arg(&root)
        .output()
        .expect("binary runs");
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("observation member path")
            && stderr.contains("cannot be represented as Unicode"),
        "{stderr}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[cfg(target_os = "linux")]
#[test]
fn native_build_rejects_non_unicode_storage_workspace_path() {
    use std::os::unix::ffi::OsStringExt;

    let root = std::env::temp_dir().join(format!(
        "topaz_non_unicode_storage_workspace_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("test root");
    let storage = root.join(std::ffi::OsString::from_vec(b"storage-\xff".to_vec()));
    std::fs::create_dir_all(&storage).expect("invalid-byte storage root");
    let entry = root.join("main.tpz");
    let out_dir = root.join("out");
    std::fs::write(&entry, "print(\"workspace-path-ok\")\n").expect("entry source");

    let built = rust_topaz()
        .arg("build")
        .arg(&entry)
        .args(["--language-version", "5.19", "--out-dir"])
        .arg(&out_dir)
        .env("TOPAZ_STORAGE_DIR", &storage)
        .output()
        .expect("binary runs");
    assert!(!built.status.success(), "{built:?}");
    let stderr = String::from_utf8_lossy(&built.stderr);
    assert!(
        stderr.contains("Topaz build storage path cannot be represented as Unicode")
            && stderr.contains("Cargo requires Unicode package paths"),
        "{stderr}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[cfg(target_os = "linux")]
#[test]
fn dev_default_output_preserves_non_unicode_physical_package_root() {
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::{PermissionsExt, symlink};

    let root = std::env::current_dir()
        .expect("current directory")
        .join(format!("topaz_dev_non_unicode_root_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("test root");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
        .expect("private test root");
    let physical = root.join(std::ffi::OsString::from_vec(b"package-\xff".to_vec()));
    std::fs::create_dir_all(physical.join("src")).expect("physical package source root");
    std::fs::write(
        physical.join("topaz.toml"),
        "[package]\nname = \"dev_exact_path\"\nversion = \"0.1.0\"\nlanguage = \"5.19\"\nentry = \"src/main.tpz\"\n\n[build]\ntarget = \"web-app\"\ndeterministic = true\n\n[web]\ntitle = \"Exact path\"\nstyles = []\nassets = []\nlifecycle = \"v2\"\n\n[dependencies]\nstd = \"5.19\"\n",
    )
    .expect("package manifest");
    let entry = physical.join("src/main.tpz");
    let initial_source = "import std.dom { Html, WebAppEvent, WebAppStep, text }\n\nexport record Model {\n  message: string,\n}\n\nexport enum Msg {\n  Ready,\n}\n\nexport function init() -> WebAppStep<Model, Msg> {\n  WebAppStep { model: Model { message: \"exact path\" }, commands: [] }\n}\n\nexport function update(model: Model, message: Msg, event: WebAppEvent) -> WebAppStep<Model, Msg> {\n  WebAppStep { model: model, commands: [] }\n}\n\nexport function view(model: Model) -> Html<Msg> {\n  text(model.message)\n}\n";
    std::fs::write(&entry, initial_source).expect("entry source");
    let pre_epoch = std::time::SystemTime::UNIX_EPOCH
        .checked_sub(Duration::from_secs(86_400))
        .expect("pre-epoch timestamp");
    std::fs::File::open(&entry)
        .expect("entry timestamp handle")
        .set_times(std::fs::FileTimes::new().set_modified(pre_epoch))
        .expect("pre-epoch entry timestamp");
    let logical = root.join("package");
    symlink(&physical, &logical).expect("logical package symlink");
    let storage = root.join("storage");
    std::fs::create_dir_all(&storage).expect("build storage");
    let port = unused_loopback_port();
    let mut dev = rust_topaz()
        .arg("dev")
        .arg("--root")
        .arg(&logical)
        .arg("--port")
        .arg(port.to_string())
        .env("TOPAZ_STORAGE_DIR", &storage)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("dev process starts");

    let deadline = Instant::now() + Duration::from_secs(60);
    let response = loop {
        if dev.try_wait().expect("dev process state").is_some() {
            break None;
        }
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .expect("dev response timeout");
                write!(
                    stream,
                    "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
                )
                .expect("dev request");
                let mut response = String::new();
                stream.read_to_string(&mut response).expect("dev response");
                break Some(response);
            }
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => break None,
        }
    };
    let replacement = root.join("replacement.tpz");
    std::fs::write(
        &replacement,
        initial_source.replace("exact path", "exact path rebuilt"),
    )
    .expect("replacement source");
    std::fs::File::open(&replacement)
        .expect("replacement timestamp handle")
        .set_times(std::fs::FileTimes::new().set_modified(pre_epoch))
        .expect("pre-epoch replacement timestamp");
    std::fs::rename(&replacement, &entry).expect("atomic source replacement");

    let rebuild_deadline = Instant::now() + Duration::from_secs(60);
    let rebuilt = loop {
        if dev.try_wait().expect("dev process state").is_some() {
            break false;
        }
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
            if write!(
                stream,
                "GET /__topaz_version HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
            )
            .is_ok()
            {
                let mut version = String::new();
                if stream.read_to_string(&mut version).is_ok()
                    && version.ends_with("\r\n\r\n2")
                {
                    break true;
                }
            }
        }
        if Instant::now() >= rebuild_deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(25));
    };

    terminate_process_tree(dev.id());
    let _ = dev.kill();
    let output = dev.wait_with_output().expect("dev process reaped");
    let response = response.unwrap_or_else(|| panic!("dev server did not start: {output:?}"));
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("topaz-app.js"), "{response}");
    assert!(rebuilt, "{output:?}");
    assert!(
        physical
            .join(".topaz/dev/web-app/topaz-artifact.json")
            .is_file(),
        "{output:?}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compiler_preview_is_typed_terminal_source_free_and_has_no_language_fallback() {
    let root = std::env::temp_dir().join(format!("topaz_compiler_preview_{}", std::process::id()));
    let output_root =
        std::env::temp_dir().join(format!("topaz_compiler_preview_out_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&output_root);
    std::fs::create_dir_all(root.join("src")).expect("source root");
    std::fs::create_dir_all(&output_root).expect("output root");
    std::fs::write(
        root.join("topaz.toml"),
        r#"[package]
name = "compiler_preview"
version = "0.1.0"
language = "5.17"
entry = "src/main.tpz"

[build]
target = "native"
deterministic = true

[dependencies]
std = "5.17"
"#,
    )
    .expect("manifest");
    std::fs::write(
        root.join("src/main.tpz"),
        "let name = \"세계\"\nlet message = \"안녕 {name}\"\nprint(message)\n",
    )
    .expect("entry");

    let inside = root.join("preview");
    let refused = topaz()
        .args(["compiler", "preview", "--out-dir"])
        .arg(&inside)
        .current_dir(&root)
        .output()
        .expect("binary runs");
    assert!(!refused.status.success(), "{refused:?}");
    assert!(!inside.exists());

    let observation = output_root.join("observation");
    let preview = topaz()
        .args(["compiler", "preview", "--out-dir"])
        .arg(&observation)
        .current_dir(&root)
        .output()
        .expect("binary runs");
    assert!(preview.status.success(), "{preview:?}");
    let response = std::fs::read_to_string(observation.join("response.json")).expect("response");
    assert!(response.contains("\"highestCompletedPhase\":\"typed\""));
    assert!(response.contains("\"ast\":\"produced\""));
    assert!(response.contains("\"resolved\":\"produced\""));
    assert!(response.contains("\"typed\":\"produced\""));
    assert!(
        std::fs::metadata(observation.join("resolved.jsonl"))
            .expect("resolved projection")
            .len()
            > 0
    );
    assert!(
        std::fs::metadata(observation.join("typed.jsonl"))
            .expect("typed projection")
            .len()
            > 0
    );
    let provenance =
        std::fs::read_to_string(observation.join("provenance.json")).expect("provenance");
    assert!(provenance.contains("\"engine\":\"topaz-front-end-preview\""));
    assert!(provenance.contains("\"defaultEngine\":\"rust-stage0\""));
    assert!(provenance.contains("\"producerStage\":0"));
    assert!(provenance.contains("\"resultStage\":0"));

    let current_manifest =
        std::fs::read_to_string(root.join("topaz.toml")).expect("current manifest");
    let inherited_manifest = current_manifest
        .replace("language = \"5.17\"", "language = \"5.16\"")
        .replace("std = \"5.17\"", "std = \"5.16\"");
    std::fs::write(root.join("topaz.toml"), inherited_manifest)
        .expect("inherited-profile manifest");
    let inherited = topaz()
        .args(["compiler", "preview", "--out-dir"])
        .arg(output_root.join("inherited-profile"))
        .current_dir(&root)
        .output()
        .expect("binary runs");
    assert!(inherited.status.success(), "{inherited:?}");
    std::fs::write(root.join("topaz.toml"), &current_manifest).expect("restore current manifest");

    let old_manifest = std::fs::read_to_string(root.join("topaz.toml"))
        .expect("current manifest")
        .replace("language = \"5.17\"", "language = \"5.14\"")
        .replace("std = \"5.17\"", "std = \"5.14\"");
    std::fs::write(root.join("topaz.toml"), old_manifest).expect("old-mode manifest");
    let package_wrong_mode = topaz()
        .args(["compiler", "preview", "--out-dir"])
        .arg(output_root.join("package-wrong-mode"))
        .current_dir(&root)
        .output()
        .expect("binary runs");
    assert!(
        !package_wrong_mode.status.success(),
        "{package_wrong_mode:?}"
    );
    assert!(
        String::from_utf8_lossy(&package_wrong_mode.stderr)
            .contains("requires a language profile admitted by the self-hosted compiler"),
        "{package_wrong_mode:?}"
    );

    std::fs::remove_dir_all(&root).expect("remove source checkout");
    let validate = topaz()
        .args(["compiler", "validate"])
        .arg(&observation)
        .output()
        .expect("binary runs");
    assert!(validate.status.success(), "{validate:?}");

    let wrong_mode = topaz()
        .args([
            "compiler",
            "preview",
            "missing.tpz",
            "--language-version",
            "5.10",
            "--out-dir",
        ])
        .arg(output_root.join("wrong-mode"))
        .output()
        .expect("binary runs");
    assert!(!wrong_mode.status.success(), "{wrong_mode:?}");
    assert!(
        String::from_utf8_lossy(&wrong_mode.stderr)
            .contains("requires a language profile admitted by the self-hosted compiler"),
        "{wrong_mode:?}"
    );

    let _ = std::fs::remove_dir_all(&output_root);
}

/// Emit-coverage gate (v5.3 P0 — docs/evolution/TPZ6001-INVENTORY.md): every
/// single-module `corpus/exec` + `corpus/v5.2` fixture the checker accepts MUST
/// also lower through native emit, except a PINNED allow-list of constructs whose
/// emit is a named, deferred owner. A new check-clean-but-emit-fail construct (the
/// `containsKey` bug class — `topaz run` works but `topaz build` fails TPZ6001)
/// fails HERE instead of slipping past the interpreter-only exec gate.
#[test]
fn checked_corpus_fixtures_emit_except_the_pinned_allowlist() {
    // Constructs with a named emit owner in TPZ6001-INVENTORY.md (repo-relative).
    // Empty means the checked single-module v5.2+ corpus is native-emit complete.
    const EMIT_DEFERRED: &[&str] = &[];

    fn collect_tpz(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_tpz(&p, out);
            } else if p.extension().is_some_and(|x| x == "tpz") {
                out.push(p);
            }
        }
    }

    let mut files = Vec::new();
    for dir in ["corpus/exec", "corpus/v5.2"] {
        collect_tpz(&repo_root().join(dir), &mut files);
    }

    let mut emit_failed = Vec::new();
    for f in &files {
        let rel = f
            .strip_prefix(repo_root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        // Multi-module units emit via their entry, not file-by-file.
        if rel.contains("/modules/") || rel.contains("m085") {
            continue;
        }
        let out = topaz().arg("emit").arg(f).output().expect("binary runs");
        if out.status.success() {
            continue;
        }
        // Only TPZ6001 (native-emit-unsupported) counts; a check-rejected fixture
        // fails with its own check code and is not an emit-coverage gap.
        if String::from_utf8_lossy(&out.stderr).contains("TPZ6001") {
            emit_failed.push(rel);
        }
    }
    emit_failed.sort();

    let mut expected: Vec<String> = EMIT_DEFERRED.iter().map(|s| s.to_string()).collect();
    expected.sort();

    assert_eq!(
        emit_failed, expected,
        "emit-coverage drift: a corpus fixture check-passes but fails native emit \
         (TPZ6001) outside the pinned allow-list. Implement its emit lowering, or add \
         a named owner to docs/evolution/TPZ6001-INVENTORY.md and EMIT_DEFERRED."
    );
}

#[test]
fn checked_v54_package_corpus_fixtures_emit() {
    let root = repo_root().join("corpus/v5_4/packages");
    let (_, package_fixtures) =
        corpus_extract::read_v52_manifest(&root.join("MANIFEST.toml")).expect("package manifest");
    let package_fixtures = package_fixtures
        .into_iter()
        .filter(|fixture| fixture.phase == "package-check" && fixture.result == "ok")
        .collect::<Vec<_>>();

    assert!(
        !package_fixtures.is_empty(),
        "expected at least one ok package-check fixture in corpus/v5_4/packages"
    );

    let out_root =
        std::env::temp_dir().join(format!("topaz_v54_package_emit_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_root);
    std::fs::create_dir_all(&out_root).expect("create package emit temp root");

    let mut failures = Vec::new();
    for fixture in &package_fixtures {
        let package_root = root.join(&fixture.dir);
        for backend in ["boxed", "native"] {
            let out_dir = out_root.join(format!(
                "{}_{backend}",
                fixture.dir.replace(['/', '\\'], "_")
            ));
            let mut cmd = topaz();
            cmd.arg("emit")
                .arg("--backend")
                .arg(backend)
                .arg("--root")
                .arg(&package_root)
                .arg("--out-dir")
                .arg(&out_dir);
            if fixture.status == corpus_extract::V52FixtureStatus::Locked {
                cmd.arg("--locked");
            }
            let out = cmd.output().expect("binary runs");
            if out.status.success() {
                continue;
            }
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let kind = if stderr.contains("TPZ6001") {
                "TPZ6001"
            } else {
                "unexpected"
            };
            failures.push(format!(
                "{} {backend} ({kind})\nstdout:\n{stdout}\nstderr:\n{stderr}",
                fixture.dir
            ));
        }
    }

    let _ = std::fs::remove_dir_all(&out_root);
    assert!(
        failures.is_empty(),
        "v5.4 package corpus emit gaps:\n{}",
        failures.join("\n\n")
    );
}

#[test]
fn checked_v54_source_gate_rows_emit() {
    const EMIT_DEFERRED: &[&str] = &[];

    let root = repo_root().join("corpus/v5_4");
    let mut source_fixtures = Vec::new();
    for area in ["check", "performance-smoke"] {
        let (_, fixtures) =
            corpus_extract::read_v52_manifest(&root.join(area).join("MANIFEST.toml"))
                .expect("manifest");
        source_fixtures.extend(
            fixtures
                .into_iter()
                .filter(|fixture| {
                    fixture.result == "ok"
                        && matches!(fixture.phase.as_str(), "check" | "performance-smoke")
                })
                .map(|fixture| (area, fixture)),
        );
    }

    assert!(
        !source_fixtures.is_empty(),
        "expected at least one ok v5.4 source gate fixture"
    );

    let mut tpz6001 = Vec::new();
    let mut unexpected = Vec::new();
    for (area, fixture) in &source_fixtures {
        let rel = format!("corpus/v5_4/{area}/{}", fixture.file);
        let path = repo_root().join(&rel);
        for backend in ["boxed", "native"] {
            let out = topaz()
                .arg("emit")
                .arg("--backend")
                .arg(backend)
                .arg(&path)
                .output()
                .expect("binary runs");
            if out.status.success() {
                continue;
            }
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.contains("TPZ6001") {
                tpz6001.push(format!("{rel} [{backend}]"));
            } else {
                unexpected.push(format!(
                    "{rel} [{backend}]\nstdout:\n{stdout}\nstderr:\n{stderr}"
                ));
            }
        }
    }
    tpz6001.sort();

    let mut expected: Vec<String> = EMIT_DEFERRED.iter().map(|s| s.to_string()).collect();
    expected.sort();

    assert!(
        unexpected.is_empty(),
        "unexpected v5.4 source gate emit failures:\n{}",
        unexpected.join("\n\n")
    );
    assert_eq!(
        tpz6001, expected,
        "v5.4 source gate TPZ6001 drift: add a named owner to \
         docs/evolution/TPZ6001-INVENTORY.md and EMIT_DEFERRED, or lower it."
    );
}
