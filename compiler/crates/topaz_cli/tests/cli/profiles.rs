use super::support::*;

#[test]
fn check_format_json_emits_one_json_object_per_diagnostic() {
    // A program with a stable check error (unknown member, TPZ5006).
    let path = std::env::temp_dir().join("topaz_cli_format_json.tpz");
    std::fs::write(&path, "let s = Set.of(1)\nlet b = s.nope(2)\n").expect("write temp");
    let out = rust_topaz()
        .arg("check")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .output()
        .expect("binary runs");
    let _ = std::fs::remove_file(&path);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Structural assertions (robust to message wording): a single-line JSON object
    // with the stable fields and a resolved 1-based position.
    assert!(
        stderr.lines().any(|l| {
            l.starts_with("{\"code\":\"TPZ")
                && l.contains("\"severity\":\"error\"")
                && l.contains("\"primary\":{\"file\":")
                && l.contains("\"lo\":")
                && l.trim_end().ends_with('}')
        }),
        "expected a JSON diagnostic line, got:\n{stderr}"
    );
    // In JSON mode stderr is a pure JSONL stream: no human summary lines.
    assert!(
        stderr
            .lines()
            .filter(|l| !l.trim().is_empty())
            .all(|l| l.starts_with('{') && l.trim_end().ends_with('}')),
        "stderr should be pure JSONL, got:\n{stderr}"
    );
}

#[test]
fn profile_json_rejects_composition_with_stable_contract() {
    let path = std::env::temp_dir().join(format!(
        "topaz_cli_profile_composition_{}.tpz",
        std::process::id()
    ));
    std::fs::write(
        &path,
        "let inc = (x: int) => x + 1\nlet twice = (x: int) => x * 2\nlet both = inc >> twice\n",
    )
    .expect("write temp");

    let ordinary = topaz()
        .arg("check")
        .arg(&path)
        .output()
        .expect("binary runs");
    assert!(ordinary.status.success(), "{ordinary:?}");

    let out = topaz()
        .arg("check")
        .arg("--profile")
        .arg("agent-pack")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .output()
        .expect("binary runs");
    let _ = std::fs::remove_file(&path);
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<_> = stderr.lines().collect();
    assert_eq!(lines.len(), 1, "{stderr}");
    assert!(
        lines[0].starts_with(
            "{\"schema\":\"topaz.profile-diagnostic/v1\",\"profile\":\"agent-pack\",\"rule\":\"agent-pack/no-composition\""
        ),
        "{stderr}"
    );
    assert!(lines[0].contains("\"code\":\"TPZ5801\""), "{stderr}");
    assert!(lines[0].contains("\"fix\":null"), "{stderr}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "{\"schema\":\"topaz.profile-check/v1\",\"profile\":\"agent-pack\",\"language\":\"topaz-5.20\",\"status\":\"fail\",\"diagnosticCount\":1,\"errorCount\":1}"
    );
}

#[test]
fn profile_assert_override_and_imported_module_scan_are_explicit() {
    let dir =
        std::env::temp_dir().join(format!("topaz_cli_profile_package_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let main = dir.join("main.tpz");
    let lib = dir.join("lib.tpz");
    std::fs::write(&main, "import lib { runChecks }\nrunChecks()\n").expect("write main");
    std::fs::write(
        &lib,
        "export function runChecks() -> () {\n    assert(true, \"ok\")\n}\n",
    )
    .expect("write lib");

    let agent = topaz()
        .arg("check")
        .arg("--root")
        .arg(&dir)
        .arg("--profile")
        .arg("agent-pack")
        .arg(&main)
        .output()
        .expect("binary runs");
    assert!(!agent.status.success(), "{agent:?}");
    let agent_stderr = String::from_utf8_lossy(&agent.stderr);
    assert!(
        agent_stderr.contains("profile[agent-pack]"),
        "{agent_stderr}"
    );
    assert!(
        agent_stderr.contains("agent-pack/no-assert"),
        "{agent_stderr}"
    );
    assert!(agent_stderr.contains("--> lib.tpz:2:5"), "{agent_stderr}");

    let tests = topaz()
        .arg("check")
        .arg("--root")
        .arg(&dir)
        .arg("--profile")
        .arg("test-profile")
        .arg(&main)
        .output()
        .expect("binary runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(tests.status.success(), "{tests:?}");
    assert!(
        String::from_utf8_lossy(&tests.stdout).contains("profile[test-profile]"),
        "{tests:?}"
    );
}

#[test]
fn profile_resolved_user_assert_is_not_the_test_builtin_in_both_compilers() {
    let path = std::env::temp_dir().join(format!(
        "topaz_cli_profile_user_assert_{}.tpz",
        std::process::id()
    ));
    std::fs::write(
        &path,
        "function assert(value: bool) -> bool { value }\nassert(true)\n",
    )
    .expect("write user assert source");

    for compiler in ["rust", "self"] {
        let checked = topaz()
            .arg("check")
            .arg("--profile")
            .arg("agent-pack")
            .args(["--compiler", compiler])
            .arg(&path)
            .output()
            .expect("profile check runs");
        assert!(checked.status.success(), "{compiler}: {checked:?}");
        assert!(
            !String::from_utf8_lossy(&checked.stderr).contains("agent-pack/no-assert"),
            "{compiler}: {checked:?}"
        );
    }

    let _ = std::fs::remove_file(path);
}

#[test]
fn profile_json_wraps_compiler_diagnostic_with_machine_fix() {
    let path =
        std::env::temp_dir().join(format!("topaz_cli_profile_fix_{}.tpz", std::process::id()));
    std::fs::write(
        &path,
        "let answer: int = 1\nfunction main() -> int { answr }\n",
    )
    .expect("write temp");
    let out = rust_topaz()
        .arg("check")
        .arg("--profile")
        .arg("agent-pack")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .output()
        .expect("binary runs");
    let _ = std::fs::remove_file(&path);
    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("\"rule\":null"), "{stderr}");
    assert!(stderr.contains("\"code\":\"TPZ5002\""), "{stderr}");
    assert!(
        stderr.contains("\"applicability\":\"machine-applicable\""),
        "{stderr}"
    );
    assert!(stderr.contains("\"replacement\":\"answer\""), "{stderr}");
}

#[test]
fn profile_flag_rejects_unsupported_contexts() {
    for args in [
        vec!["run", "--profile", "agent-pack", "whatever.tpz"],
        vec![
            "check",
            "--profile",
            "agent-pack",
            "--language-version",
            "5.5",
            "whatever.tpz",
        ],
        vec![
            "check",
            "--profile",
            "agent-pack",
            "--exports-json",
            "whatever.tpz",
        ],
        vec!["check", "--profile", "public-docs", "whatever.tpz"],
    ] {
        let out = topaz().args(args).output().expect("binary runs");
        assert!(!out.status.success(), "{out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("--profile"),
            "{out:?}"
        );
    }
}

#[test]
fn bootstrap_profile_accepts_a_locked_multimodule_package_and_reports_package_boundaries() {
    let dir = std::env::temp_dir().join(format!(
        "topaz_cli_bootstrap_profile_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("create package source");
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
"#;
    std::fs::write(dir.join("topaz.toml"), manifest).expect("manifest");
    std::fs::write(dir.join("topaz.lock"), package_lock(manifest)).expect("lock");
    std::fs::write(
        dir.join("src/main.tpz"),
        "import src.math { plus }\nlet answer = plus(20, 22)\n",
    )
    .expect("entry");
    std::fs::write(
        dir.join("src/math.tpz"),
        "export function plus(a: int, b: int) -> int { a + b }\n",
    )
    .expect("module");

    let accepted = rust_topaz()
        .arg("check")
        .arg("--profile")
        .arg("bootstrap")
        .arg("--locked")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(accepted.status.success(), "{accepted:?}");
    assert!(
        String::from_utf8_lossy(&accepted.stdout).contains("profile[bootstrap]"),
        "{accepted:?}"
    );

    let unlocked = rust_topaz()
        .arg("check")
        .arg("--profile")
        .arg("bootstrap")
        .arg("--format")
        .arg("json")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    assert!(!unlocked.status.success(), "{unlocked:?}");
    assert!(
        String::from_utf8_lossy(&unlocked.stderr)
            .contains("\"rule\":\"bootstrap/requires-locked-package\""),
        "{unlocked:?}"
    );

    let mut nondeterministic = manifest.replace("deterministic = true", "deterministic = false");
    nondeterministic.push_str("\n[capabilities.fs]\nread = [\"input\"]\n");
    std::fs::write(dir.join("topaz.toml"), &nondeterministic).expect("boundary manifest");
    std::fs::write(dir.join("topaz.lock"), package_lock(&nondeterministic)).expect("boundary lock");
    let boundary = rust_topaz()
        .arg("check")
        .arg("--profile")
        .arg("bootstrap")
        .arg("--format")
        .arg("json")
        .arg("--locked")
        .arg("--root")
        .arg(&dir)
        .output()
        .expect("binary runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!boundary.status.success(), "{boundary:?}");
    let stderr = String::from_utf8_lossy(&boundary.stderr);
    assert!(
        stderr.contains("\"rule\":\"bootstrap/requires-deterministic-build\""),
        "{stderr}"
    );
    assert!(
        stderr.contains("\"rule\":\"bootstrap/no-capability\""),
        "{stderr}"
    );
}

#[test]
fn format_json_is_rejected_outside_check() {
    let out = topaz()
        .arg("run")
        .arg("--format")
        .arg("json")
        .arg("whatever.tpz")
        .output()
        .expect("binary runs");
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("applies to `check` only"),
        "{out:?}"
    );
}

#[test]
fn explain_renders_human_diagnostic_entry() {
    let out = topaz()
        .arg("explain")
        .arg("TPZ5602")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("TPZ5602: Duplicate key in map literal"),
        "{stdout}"
    );
    assert!(
        stdout.contains("machine.kind: duplicate_map_key"),
        "{stdout}"
    );
    assert!(stdout.contains("fix-it:"), "{stdout}");
}

#[test]
fn explain_json_renders_stable_machine_object() {
    let out = topaz()
        .arg("explain")
        .arg("TPZ5522")
        .arg("--json")
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.trim();
    assert!(line.starts_with("{\"code\":\"TPZ5522\""), "{stdout}");
    assert!(
        line.contains("\"machine\":{\"kind\":\"missing_protocol_conformance\"}"),
        "{stdout}"
    );
    assert!(line.contains("\"fixits\":["), "{stdout}");
}

#[test]
fn explain_rejects_unknown_or_malformed_codes() {
    let malformed = topaz()
        .arg("explain")
        .arg("TPZ56A2")
        .output()
        .expect("binary runs");
    assert!(!malformed.status.success(), "{malformed:?}");
    assert!(
        String::from_utf8_lossy(&malformed.stderr).contains("shape TPZ####"),
        "{malformed:?}"
    );

    let unknown = topaz()
        .arg("explain")
        .arg("TPZ9999")
        .output()
        .expect("binary runs");
    assert!(!unknown.status.success(), "{unknown:?}");
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("no explanation registered"),
        "{unknown:?}"
    );
}
