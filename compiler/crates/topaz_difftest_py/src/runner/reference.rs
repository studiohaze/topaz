use crate::*;

pub(crate) fn build_reference(src: &Path, bin: &Path) -> Result<PathBuf, String> {
    let output = Command::new("rustc")
        .arg("-O")
        .arg(src)
        .arg("-o")
        .arg(bin)
        .output()
        .map_err(|e| format!("spawn rustc: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "reference build failed for {}:\n{}",
            src.display(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(bin.to_path_buf())
}

pub(crate) fn run_reference_bin(bin: &Path, input: &str) -> Result<String, String> {
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn reference: {e}"))?;
    child
        .stdin
        .as_mut()
        .expect("reference stdin")
        .write_all(input.as_bytes())
        .map_err(|e| format!("write reference stdin: {e}"))?;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("wait reference: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "reference exited nonzero\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if !output.stderr.is_empty() {
        return Err(format!(
            "reference wrote stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("utf8 reference stdout: {e}"))
}
