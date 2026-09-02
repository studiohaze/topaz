//! Installed-command integration for the exact bounded Lispex evaluator.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};

use topaz_lispex_embed::{LIMITS_FILE_MAX_BYTES, Limits, RunError, SettledCategory};

static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

const USAGE: &str = "\
topaz lispex embed run --source <rule.lspx> --input <value.lpxvalue> \
--limits <limits.json> --output <new-directory>
topaz lispex embed info --json";

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunOptions {
    source: PathBuf,
    input: PathBuf,
    limits: PathBuf,
    output: PathBuf,
}

#[derive(Debug)]
enum CommandError {
    Usage(String),
    Refusal(String),
    Engine(String),
}

/// Dispatches the restricted `lispex embed` command surface.
pub fn dispatch(args: &[String]) -> ExitCode {
    match args {
        [command, flag] if command == "info" && flag == "--json" => {
            match topaz_lispex_embed::info_json() {
                Ok(info) => {
                    println!("{info}");
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("topaz: lispex embed info failed: {error}");
                    ExitCode::from(4)
                }
            }
        }
        [command, rest @ ..] if command == "run" => match parse_run_options(rest) {
            Ok(options) => match execute(&options) {
                Ok(_) => ExitCode::SUCCESS,
                Err(CommandError::Usage(error)) => {
                    eprintln!("topaz: {error}\n\n{USAGE}");
                    ExitCode::from(2)
                }
                Err(CommandError::Refusal(error)) => {
                    eprintln!("topaz: {error}");
                    ExitCode::from(3)
                }
                Err(CommandError::Engine(error)) => {
                    eprintln!("topaz: {error}");
                    ExitCode::from(4)
                }
            },
            Err(error) => {
                eprintln!("topaz: {error}\n\n{USAGE}");
                ExitCode::from(2)
            }
        },
        _ => {
            eprintln!("topaz: wrong arguments for `lispex embed`\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn parse_run_options(args: &[String]) -> Result<RunOptions, String> {
    let mut source = None;
    let mut input = None;
    let mut limits = None;
    let mut output = None;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let slot = match flag {
            "--source" => &mut source,
            "--input" => &mut input,
            "--limits" => &mut limits,
            "--output" => &mut output,
            _ => return Err(format!("unknown `lispex embed run` argument `{flag}`")),
        };
        if slot.is_some() {
            return Err(format!("duplicate `lispex embed run` argument `{flag}`"));
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("`{flag}` requires a path"))?;
        if value.is_empty() || value.starts_with("--") {
            return Err(format!("`{flag}` requires a path"));
        }
        *slot = Some(PathBuf::from(value));
        index += 2;
    }
    Ok(RunOptions {
        source: source.ok_or_else(|| "missing `--source`".to_string())?,
        input: input.ok_or_else(|| "missing `--input`".to_string())?,
        limits: limits.ok_or_else(|| "missing `--limits`".to_string())?,
        output: output.ok_or_else(|| "missing `--output`".to_string())?,
    })
}

fn execute(options: &RunOptions) -> Result<SettledCategory, CommandError> {
    let mut publication = OutputTransaction::prepare(&options.output)?;
    let limits_bytes = read_regular_file(
        &options.limits,
        LIMITS_FILE_MAX_BYTES,
        "limits",
        CommandClass::Usage,
    )?;
    let limits_text = std::str::from_utf8(&limits_bytes)
        .map_err(|_| CommandError::Usage("limits file is not UTF-8".into()))?;
    let limits = Limits::parse_json(limits_text)
        .map_err(|error| CommandError::Usage(format!("invalid limits file: {error}")))?;
    let source = read_regular_file(
        &options.source,
        limits.prepare.raw_source_bytes,
        "source",
        CommandClass::Refusal,
    )?;
    let input = read_regular_file(
        &options.input,
        limits.evaluate.canonical_input_bytes,
        "input",
        CommandClass::Refusal,
    )?;
    let record = topaz_lispex_embed::run(&source, &input, limits).map_err(map_run_error)?;
    if let Some(result) = record.result.as_deref() {
        publication.write_file("result.lpxvalue", result)?;
    }
    publication.write_file("report.json", record.report_json.as_bytes())?;
    publication.commit()?;
    Ok(record.category)
}

#[derive(Clone, Copy)]
enum CommandClass {
    Usage,
    Refusal,
}

fn read_regular_file(
    path: &Path,
    maximum: u64,
    label: &str,
    class: CommandClass,
) -> Result<Vec<u8>, CommandError> {
    let error = |message: String| match class {
        CommandClass::Usage => CommandError::Usage(message),
        CommandClass::Refusal => CommandError::Refusal(message),
    };
    let metadata = fs::symlink_metadata(path).map_err(|cause| {
        error(format!(
            "cannot inspect {label} `{}`: {cause}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(error(format!(
            "{label} `{}` is not a named regular file",
            path.display()
        )));
    }
    if metadata.len() > maximum {
        return Err(error(format!(
            "{label} `{}` is {} bytes, exceeding limit {maximum}",
            path.display(),
            metadata.len()
        )));
    }
    let mut file = File::open(path)
        .map_err(|cause| error(format!("cannot open {label} `{}`: {cause}", path.display())))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|cause| error(format!("cannot read {label} `{}`: {cause}", path.display())))?;
    if bytes.len() as u64 > maximum {
        return Err(error(format!(
            "{label} `{}` changed while reading and exceeds limit {maximum}",
            path.display()
        )));
    }
    Ok(bytes)
}

fn map_run_error(error: RunError) -> CommandError {
    match error {
        RunError::InputRefusal(_) | RunError::RequestRefusal(_) => {
            CommandError::Refusal(error.to_string())
        }
        RunError::SelectionRefusal(_)
        | RunError::ContractViolation(_)
        | RunError::EngineFault(_)
        | RunError::Operational(_) => CommandError::Engine(error.to_string()),
    }
}

struct OutputTransaction {
    destination: PathBuf,
    staging: PathBuf,
    active: bool,
}

impl OutputTransaction {
    fn prepare(destination: &Path) -> Result<Self, CommandError> {
        let name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                CommandError::Refusal(format!(
                    "output `{}` needs a UTF-8 directory name",
                    destination.display()
                ))
            })?;
        let parent = destination
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = fs::canonicalize(parent).map_err(|cause| {
            CommandError::Refusal(format!(
                "cannot resolve output parent `{}`: {cause}",
                parent.display()
            ))
        })?;
        let destination = parent.join(name);
        ensure_absent(&destination)?;
        for _ in 0..32 {
            let nonce = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);
            let staging = parent.join(format!(
                ".{name}.topaz-lispex-embed-{}-{nonce}",
                std::process::id()
            ));
            match fs::create_dir(&staging) {
                Ok(()) => {
                    set_directory_permissions(&staging)?;
                    return Ok(Self {
                        destination,
                        staging,
                        active: true,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(CommandError::Refusal(format!(
                        "cannot create output staging directory: {error}"
                    )));
                }
            }
        }
        Err(CommandError::Refusal(
            "cannot reserve a unique output staging directory".into(),
        ))
    }

    fn write_file(&self, name: &str, bytes: &[u8]) -> Result<(), CommandError> {
        let path = self.staging.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|cause| {
                CommandError::Refusal(format!(
                    "cannot create staged output `{}`: {cause}",
                    path.display()
                ))
            })?;
        set_file_permissions(&path)?;
        file.write_all(bytes).map_err(|cause| {
            CommandError::Refusal(format!(
                "cannot write staged output `{}`: {cause}",
                path.display()
            ))
        })?;
        file.sync_all().map_err(|cause| {
            CommandError::Refusal(format!(
                "cannot synchronize staged output `{}`: {cause}",
                path.display()
            ))
        })
    }

    fn commit(&mut self) -> Result<(), CommandError> {
        sync_directory(&self.staging)?;
        publish_no_replace(&self.staging, &self.destination).map_err(|cause| {
            CommandError::Refusal(format!(
                "cannot publish output directory `{}` without replacement: {cause}",
                self.destination.display()
            ))
        })?;
        self.active = false;
        Ok(())
    }
}

impl Drop for OutputTransaction {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_dir_all(&self.staging);
        }
    }
}

fn ensure_absent(path: &Path) -> Result<(), CommandError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(CommandError::Refusal(format!(
            "output destination `{}` already exists",
            path.display()
        ))),
        Err(error) => Err(CommandError::Refusal(format!(
            "cannot inspect output destination `{}`: {error}",
            path.display()
        ))),
    }
}

#[cfg(unix)]
fn set_directory_permissions(path: &Path) -> Result<(), CommandError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|cause| {
        CommandError::Refusal(format!(
            "cannot set staging permissions `{}`: {cause}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path) -> Result<(), CommandError> {
    Ok(())
}

#[cfg(unix)]
fn set_file_permissions(path: &Path) -> Result<(), CommandError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|cause| {
        CommandError::Refusal(format!(
            "cannot set output permissions `{}`: {cause}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) -> Result<(), CommandError> {
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), CommandError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|cause| {
            CommandError::Refusal(format!(
                "cannot synchronize output directory `{}`: {cause}",
                path.display()
            ))
        })
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), CommandError> {
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn publish_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        source,
        rustix::fs::CWD,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(std::io::Error::from)
}

#[cfg(windows)]
fn publish_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn publish_no_replace(_source: &Path, _destination: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "no no-replace directory rename for this target",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_root(name: &str) -> PathBuf {
        let serial = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "topaz-lispex-embed-{name}-{}-{serial}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).expect("test root");
        root
    }

    fn write_inputs(root: &Path, limits: &str) -> RunOptions {
        let source = root.join("rule.lspx");
        let input = root.join("input.lpxvalue");
        let limits_path = root.join("limits.json");
        fs::write(&source, b"(if (< 10 15) \"allow\" \"deny\")\n").expect("source");
        fs::write(&input, [0]).expect("input");
        fs::write(&limits_path, limits).expect("limits");
        RunOptions {
            source,
            input,
            limits: limits_path,
            output: root.join("out"),
        }
    }

    fn maximum_limits() -> &'static str {
        "{\"schema\":\"topaz.lispex-embed-limits/v1\",\"prepare\":{\"raw_source_bytes\":4096,\"prepare_work\":1000000,\"logical_allocation\":1000000,\"syntax_depth\":64},\"evaluate\":{\"canonical_input_bytes\":4096,\"eval_work\":1000000,\"logical_allocation\":1000000,\"semantic_frames\":1000,\"traversal_depth\":256,\"output_bytes\":1000000,\"diagnostic_bytes\":1000000,\"transcript_bytes\":1000000,\"transcript_events\":100,\"result_bytes\":1000000}}"
    }

    #[test]
    fn complete_run_publishes_exact_directory_shape() {
        let root = test_root("complete");
        let options = write_inputs(&root, maximum_limits());
        assert_eq!(
            execute(&options).expect("execute"),
            SettledCategory::Complete
        );
        assert!(options.output.join("result.lpxvalue").is_file());
        assert!(options.output.join("report.json").is_file());
        assert_eq!(fs::read_dir(&options.output).expect("output").count(), 2);
        let report = fs::read_to_string(options.output.join("report.json")).expect("report");
        assert!(report.contains("\"schema\":\"topaz.lispex-embed-run-report/v4\""));
        assert!(report.contains("\"category\":\"complete\""));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn malformed_limits_publish_nothing() {
        let root = test_root("limits");
        let options = write_inputs(&root, "{\"schema\":\"wrong\"}");
        assert!(matches!(execute(&options), Err(CommandError::Usage(_))));
        assert!(!options.output.exists());
        assert_eq!(
            fs::read_dir(&root)
                .expect("root")
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().contains("topaz-lispex"))
                .count(),
            0
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn existing_output_is_never_replaced() {
        let root = test_root("collision");
        let options = write_inputs(&root, maximum_limits());
        fs::create_dir(&options.output).expect("output");
        fs::write(options.output.join("sentinel"), b"keep").expect("sentinel");
        assert!(matches!(execute(&options), Err(CommandError::Refusal(_))));
        assert_eq!(
            fs::read(options.output.join("sentinel")).expect("sentinel"),
            b"keep"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn racing_output_is_refused_by_no_replace_rename() {
        let root = test_root("race");
        let destination = root.join("out");
        let mut transaction = OutputTransaction::prepare(&destination).expect("transaction");
        transaction.write_file("report.json", b"{}").expect("file");
        fs::create_dir(&destination).expect("racing destination");
        fs::write(destination.join("sentinel"), b"keep").expect("sentinel");
        assert!(matches!(
            transaction.commit(),
            Err(CommandError::Refusal(_))
        ));
        assert_eq!(
            fs::read(destination.join("sentinel")).expect("sentinel"),
            b"keep"
        );
        drop(transaction);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn parser_rejects_duplicate_unknown_and_selector_arguments() {
        for args in [
            vec!["--source", "a", "--source", "b"],
            vec!["--source", "a", "--input", "b", "--limits", "c"],
            vec![
                "--source",
                "a",
                "--input",
                "b",
                "--limits",
                "c",
                "--output",
                "d",
                "--runtime",
                "other",
            ],
        ] {
            let args: Vec<String> = args.into_iter().map(str::to_string).collect();
            assert!(parse_run_options(&args).is_err());
        }
    }
}
