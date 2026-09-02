use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn topaz() -> Command {
    Command::new(env!("CARGO_BIN_EXE_topaz"))
}

fn compiler_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn source() -> PathBuf {
    compiler_root().join("lit/h0/lit-kernel-h0.tpz")
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(compiler_root().join(format!("lit/h0/fixtures/{name}.json")))
        .unwrap_or_else(|error| panic!("read LIT H0 fixture {name}: {error}"))
}

fn output_with_stdin(mut command: Command, stdin: &str) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("command spawns");
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .expect("stdin writes");
    child.wait_with_output().expect("command completes")
}

fn stdout_line(output: &Output, label: &str) -> String {
    assert!(output.status.success(), "{label}: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.is_empty(), "{label} stderr: {stderr}");
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string()
}

fn native_program(out_dir: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    };
    out_dir.join("target/debug").join(name)
}

fn cpython_31314() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("TOPAZ_PYTHON_31314") {
        candidates.push(PathBuf::from(path));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/python3.13"));
    candidates.push(PathBuf::from("python3.13"));
    for candidate in candidates {
        let Ok(output) = Command::new(&candidate)
            .arg("-c")
            .arg("import sys; print(sys.version.split()[0]); print(sys.implementation.cache_tag)")
            .output()
        else {
            continue;
        };
        if output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .lines()
                .eq(["3.13.14", "cpython-313"])
        {
            return Some(candidate);
        }
    }
    None
}

#[test]
fn lit_h0_source_is_checker_clean() {
    let output = topaz()
        .arg("check")
        .arg(source())
        .output()
        .expect("Topaz checker runs");
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("types-ok (1 module)"), "{stdout}");
    assert!(stdout.contains("resolve-ok (1 module)"), "{stdout}");
}

#[test]
#[ignore = "long-running cross-host and negative campaign; run explicitly"]
fn lit_h0_kernel_rehearsal_campaign() {
    let temp = std::env::temp_dir().join(format!("topaz_lit_h0_{}", std::process::id()));
    let native_out = temp.join("native");
    let python_out = temp.join("python");
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).expect("temporary LIT H0 directory");

    let build = topaz()
        .arg("build")
        .arg(source())
        .arg("--out-dir")
        .arg(&native_out)
        .output()
        .expect("native LIT H0 artifact builds");
    assert!(build.status.success(), "{build:?}");
    let program = native_program(&native_out);
    assert!(
        program.is_file(),
        "missing native artifact: {}",
        program.display()
    );

    let run_native = |name: &str| {
        let command = Command::new(&program);
        stdout_line(&output_with_stdin(command, &fixture(name)), name)
    };

    let literal = run_native("literal");
    assert!(
        literal.contains("\"h0-literal\",\"ok\",[\"one\",\"42\"]"),
        "{literal}"
    );

    let values = run_native("values-context");
    assert!(
        values.contains("\"h0-values-context\",\"ok\",[\"one\",\"3\"]"),
        "{values}"
    );

    let lexical_100 = run_native("lexical-tail-100");
    assert!(lexical_100.contains("[\"one\",\"5062\"]"), "{lexical_100}");
    assert!(
        lexical_100.contains("[\"max-continuation-depth\",6]"),
        "{lexical_100}"
    );
    let lexical_10k = run_native("lexical-tail-10000");
    assert!(
        lexical_10k.contains("[\"one\",\"50005012\"]"),
        "{lexical_10k}"
    );
    assert!(
        lexical_10k.contains("[\"max-continuation-depth\",6]"),
        "{lexical_10k}"
    );

    let reordered = run_native("lexical-tail-100-reordered");
    assert!(reordered.contains("[\"one\",\"5071\"]"), "{reordered}");
    assert_ne!(
        reordered, lexical_100,
        "operand-order positive control was not observed"
    );

    for (name, expected) in [
        ("letrec-uninitialized", "E321 uninitialized letrec binding"),
        ("values-single-context", "E320 multiple values in if test"),
        (
            "quoted-vector-immutable",
            "E312 vector-set!: immutable quoted vector",
        ),
        ("invalid-schema", "LIT106 kernel: unsupported schema"),
        (
            "unknown-opcode",
            "LIT143 kernel.nodes[0]: unknown opcode internal-closure",
        ),
        ("broken-root", "LIT162 roots[0]: broken node reference"),
        (
            "forged-internal-datum",
            "LIT112 kernel.quotes[0].datum: unknown or internal datum tag closure",
        ),
        (
            "host-path-source",
            "LIT109 kernel: source id is empty or host-path-shaped",
        ),
        (
            "missing-capability",
            "LIT151 kernel.capabilities: expected exact H0 denominator",
        ),
    ] {
        let output = run_native(name);
        assert!(output.contains(expected), "{name}: {output}");
    }

    let resource = run_native("resource");
    assert!(
        resource.contains("\"h0-resource\",\"resource\""),
        "{resource}"
    );
    assert!(
        resource.contains("logical transition budget exhausted"),
        "{resource}"
    );
    assert!(resource.contains("[\"exhausted\",true]"), "{resource}");

    for output in [&literal, &values, &lexical_100, &lexical_10k, &resource] {
        assert!(!output.contains("/Users/"), "host path leaked: {output}");
        assert!(!output.contains("\\Users\\"), "host path leaked: {output}");
    }

    let mut interpreted = topaz();
    interpreted.arg("run").arg(source());
    let interpreted = stdout_line(
        &output_with_stdin(interpreted, &fixture("lexical-tail-100")),
        "Topaz interpreter host",
    );
    assert_eq!(
        interpreted, lexical_100,
        "interpreter/Rust-artifact host drift"
    );

    if let Some(python) = cpython_31314() {
        let build = topaz()
            .arg("build")
            .arg(source())
            .arg("--target")
            .arg("python")
            .arg("--out-dir")
            .arg(&python_out)
            .output()
            .expect("Python LIT H0 artifact builds");
        assert!(build.status.success(), "{build:?}");
        let mut command = Command::new(python);
        command.current_dir(&python_out).arg("-c").arg(
            "import json,sys,program; trace=json.loads(program.run(sys.stdin.read())); sys.stdout.write('\\n'.join(trace['stdout']))",
        );
        let python_line = stdout_line(
            &output_with_stdin(command, &fixture("lexical-tail-100")),
            "Topaz Python-artifact host",
        );
        assert_eq!(python_line, lexical_100, "Python-artifact host drift");
    }

    let _ = std::fs::remove_dir_all(&temp);
}
