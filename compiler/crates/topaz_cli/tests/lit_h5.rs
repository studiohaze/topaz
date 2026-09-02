use std::path::{Path, PathBuf};
use std::process::Command;

fn topaz() -> Command {
    Command::new(env!("CARGO_BIN_EXE_topaz"))
}

fn compiler_root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn source() -> PathBuf {
    compiler_root().join("lit/lit.tpz")
}

#[test]
fn lit_h5_source_and_python_emission_are_clean() {
    let output = topaz()
        .args(["--compiler", "rust"])
        .arg("check")
        .arg(source())
        .output()
        .expect("Topaz H5 checker runs");
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("types-ok (20 modules)"), "{stdout}");
    assert!(stdout.contains("resolve-ok (20 modules)"), "{stdout}");

    let temp =
        std::env::temp_dir().join(format!("topaz_lit_h5_python_emit_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    let python = topaz()
        .args(["--compiler", "rust"])
        .arg("build")
        .arg(source())
        .arg("--target")
        .arg("python")
        .arg("--out-dir")
        .arg(&temp)
        .output()
        .expect("LIT H5 Python artifact emits");
    assert!(python.status.success(), "{python:?}");
    assert!(temp.join("program.py").is_file());
    assert!(temp.join("topaz_py_rt.py").is_file());
    let _ = std::fs::remove_dir_all(&temp);
}

#[test]
#[ignore = "long-running 144-case three-host source campaign; run explicitly"]
fn lit_h5_three_host_source_campaign() {
    let status = Command::new("node")
        .current_dir(compiler_root())
        .arg("lit/run-h5-campaign.mjs")
        .arg("--topaz")
        .arg(env!("CARGO_BIN_EXE_topaz"))
        .status()
        .expect("LIT H5 campaign supervisor runs");
    assert!(status.success(), "LIT H5 campaign failed: {status}");
}
