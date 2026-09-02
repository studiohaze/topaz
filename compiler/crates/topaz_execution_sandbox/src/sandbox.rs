use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use topaz_value::value::sha256;

use crate::protocol::{
    MAX_RESPONSE_BYTES, ProbeRequest, ProbeResponse, ProtocolError, WorkerRequest, WorkerResponse,
};

const DARWIN_SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
const DARWIN_SYSTEM_PROFILE: &str = "/System/Library/Sandbox/Profiles/system.sb";
const DARWIN_DYLD_PROFILE: &str = "/System/Library/Sandbox/Profiles/dyld-support.sb";
const SUPERVISOR_CAPTURE_LIMIT: usize = MAX_RESPONSE_BYTES + 4_096;
const DARWIN_POLICY_ID: &str = "topaz.execution-sandbox/darwin-sandbox-exec/v1";
const ADMISSION_POLICY_ID: &str = "topaz.execution-foundation/admission/v1";
const ADMISSION_POLICY_BYTES: &[u8] =
    include_bytes!("../../../contracts/execution-sandbox/v1/policy.json");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticIdentity {
    pub language_profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionEnvironmentIdentity {
    pub target: String,
    pub os_version: String,
    pub os_build: String,
    pub kernel_release: String,
    pub backend_policy: String,
    pub backend_program: String,
    pub backend_program_sha256: String,
    pub imported_system_profile: String,
    pub imported_system_profile_sha256: String,
    pub imported_dyld_profile: String,
    pub imported_dyld_profile_sha256: String,
    pub generated_profile_sha256: String,
    pub executable_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionIdentity {
    pub policy: String,
    pub policy_sha256: String,
    pub target: String,
    pub backend_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxedWorkerResult {
    pub semantic_identity: SemanticIdentity,
    pub execution_environment_identity: ExecutionEnvironmentIdentity,
    pub admission_identity: AdmissionIdentity,
    pub response: WorkerResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxedProbeResult {
    pub execution_environment_identity: ExecutionEnvironmentIdentity,
    pub admission_identity: AdmissionIdentity,
    pub response: ProbeResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxError {
    UnsupportedTarget { target: String },
    InvalidExecutablePath,
    FacilityMissing { path: String },
    IdentityRead { path: String, message: String },
    Spawn(String),
    Pipe(String),
    Timeout,
    OutputLimit { channel: &'static str, limit: usize },
    BackendRejected { status: String, stderr: String },
    Protocol(String),
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedTarget { target } => {
                write!(f, "no qualified OS sandbox backend for target `{target}`")
            }
            Self::InvalidExecutablePath => {
                f.write_str("sandbox executable path cannot be represented safely")
            }
            Self::FacilityMissing { path } => {
                write!(f, "required sandbox facility is missing: `{path}`")
            }
            Self::IdentityRead { path, message } => {
                write!(f, "cannot identify sandbox input `{path}`: {message}")
            }
            Self::Spawn(message) => write!(f, "sandbox process spawn failed: {message}"),
            Self::Pipe(message) => write!(f, "sandbox pipe failed: {message}"),
            Self::Timeout => f.write_str("sandbox worker exceeded its foundation wall guard"),
            Self::OutputLimit { channel, limit } => {
                write!(f, "sandbox {channel} exceeded {limit} bytes")
            }
            Self::BackendRejected { status, stderr } => {
                write!(f, "sandbox backend rejected execution ({status}): {stderr}")
            }
            Self::Protocol(message) => write!(f, "sandbox worker protocol failed: {message}"),
        }
    }
}

impl std::error::Error for SandboxError {}

impl From<ProtocolError> for SandboxError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value.to_string())
    }
}

pub trait PlatformSandbox {
    fn run_worker(
        &self,
        executable: &Path,
        request: &WorkerRequest,
        wall_guard: Duration,
    ) -> Result<SandboxedWorkerResult, SandboxError>;

    fn run_probe(
        &self,
        executable: &Path,
        request: &ProbeRequest,
        wall_guard: Duration,
    ) -> Result<SandboxedProbeResult, SandboxError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DarwinSandbox;

impl DarwinSandbox {
    pub fn qualify() -> Result<Self, SandboxError> {
        let target = current_target();
        if target != "aarch64-apple-darwin" {
            return Err(SandboxError::UnsupportedTarget { target });
        }
        require_file(DARWIN_SANDBOX_EXEC)?;
        require_file(DARWIN_SYSTEM_PROFILE)?;
        require_file(DARWIN_DYLD_PROFILE)?;
        Ok(Self)
    }

    pub fn profile_for(executable: &Path) -> Result<String, SandboxError> {
        let executable = executable
            .canonicalize()
            .map_err(|error| SandboxError::IdentityRead {
                path: executable.display().to_string(),
                message: error.to_string(),
            })?;
        let executable = sbpl_literal(&executable)?;
        Ok(format!(
            concat!(
                "(version 1)\n",
                "(deny default)\n",
                "(import \"system.sb\")\n",
                "(deny network*)\n",
                "(deny file-write*)\n",
                "(deny process-fork)\n",
                "(allow process-exec (literal \"{executable}\"))\n",
                "(allow file-read* file-map-executable (literal \"{executable}\"))\n"
            ),
            executable = executable
        ))
    }

    fn run_framed(
        &self,
        executable: &Path,
        frame: Vec<u8>,
        wall_guard: Duration,
    ) -> Result<(Vec<u8>, ExecutionEnvironmentIdentity, AdmissionIdentity), SandboxError> {
        let executable = executable
            .canonicalize()
            .map_err(|error| SandboxError::IdentityRead {
                path: executable.display().to_string(),
                message: error.to_string(),
            })?;
        let profile = Self::profile_for(&executable)?;
        let execution_environment_identity = execution_identity(&executable, &profile)?;
        let admission_identity = AdmissionIdentity {
            policy: ADMISSION_POLICY_ID.to_string(),
            policy_sha256: digest_bytes(ADMISSION_POLICY_BYTES),
            target: execution_environment_identity.target.clone(),
            backend_policy: DARWIN_POLICY_ID.to_string(),
        };

        let mut child = Command::new(DARWIN_SANDBOX_EXEC)
            .args(["-p", &profile, "--"])
            .arg(&executable)
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| SandboxError::Spawn(error.to_string()))?;
        let (status, stdout, stderr) = communicate_with_guard(&mut child, frame, wall_guard)?;
        if !status.success() {
            return Err(SandboxError::BackendRejected {
                status: status.to_string(),
                stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
            });
        }
        if !stderr.is_empty() {
            return Err(SandboxError::BackendRejected {
                status: status.to_string(),
                stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
            });
        }
        verify_execution_identity(&execution_environment_identity, &executable, &profile)?;
        Ok((stdout, execution_environment_identity, admission_identity))
    }
}

fn communicate_with_guard(
    child: &mut Child,
    frame: Vec<u8>,
    wall_guard: Duration,
) -> Result<(ExitStatus, Vec<u8>, Vec<u8>), SandboxError> {
    let Some(mut stdin) = child.stdin.take() else {
        terminate_and_reap(child);
        return Err(SandboxError::Pipe(
            "worker stdin is unavailable".to_string(),
        ));
    };
    let Some(stdout) = child.stdout.take() else {
        terminate_and_reap(child);
        return Err(SandboxError::Pipe(
            "worker stdout is unavailable".to_string(),
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        terminate_and_reap(child);
        return Err(SandboxError::Pipe(
            "worker stderr is unavailable".to_string(),
        ));
    };
    let stdin_writer = thread::spawn(move || {
        stdin
            .write_all(&frame)
            .map_err(|error| SandboxError::Pipe(error.to_string()))
    });
    let stdout_reader =
        thread::spawn(move || read_limited(stdout, SUPERVISOR_CAPTURE_LIMIT, "stdout"));
    let stderr_reader =
        thread::spawn(move || read_limited(stderr, SUPERVISOR_CAPTURE_LIMIT, "stderr"));

    let status = wait_with_guard(child, wall_guard);
    let stdin = join_writer(stdin_writer);
    let stdout = join_reader(stdout_reader);
    let stderr = join_reader(stderr_reader);
    let status = status?;
    stdin?;
    Ok((status, stdout?, stderr?))
}

impl PlatformSandbox for DarwinSandbox {
    fn run_worker(
        &self,
        executable: &Path,
        request: &WorkerRequest,
        wall_guard: Duration,
    ) -> Result<SandboxedWorkerResult, SandboxError> {
        let frame = request.encode()?;
        let (stdout, execution_environment_identity, admission_identity) =
            self.run_framed(executable, frame, wall_guard)?;
        let response = WorkerResponse::read_from(&mut stdout.as_slice())?;
        Ok(SandboxedWorkerResult {
            semantic_identity: SemanticIdentity {
                language_profile: "topaz-5.17".to_string(),
            },
            execution_environment_identity,
            admission_identity,
            response,
        })
    }

    fn run_probe(
        &self,
        executable: &Path,
        request: &ProbeRequest,
        wall_guard: Duration,
    ) -> Result<SandboxedProbeResult, SandboxError> {
        let frame = request.encode()?;
        let (stdout, execution_environment_identity, admission_identity) =
            self.run_framed(executable, frame, wall_guard)?;
        let response = ProbeResponse::read_from(&mut stdout.as_slice())?;
        Ok(SandboxedProbeResult {
            execution_environment_identity,
            admission_identity,
            response,
        })
    }
}

pub fn current_backend() -> Result<Box<dyn PlatformSandbox>, SandboxError> {
    Ok(Box::new(DarwinSandbox::qualify()?))
}

fn current_target() -> String {
    let arch = std::env::consts::ARCH;
    let os = match std::env::consts::OS {
        "macos" => "apple-darwin",
        other => other,
    };
    format!("{arch}-{os}")
}

fn require_file(path: &str) -> Result<(), SandboxError> {
    if Path::new(path).is_file() {
        Ok(())
    } else {
        Err(SandboxError::FacilityMissing {
            path: path.to_string(),
        })
    }
}

fn execution_identity(
    executable: &Path,
    profile: &str,
) -> Result<ExecutionEnvironmentIdentity, SandboxError> {
    let backend_program = Path::new(DARWIN_SANDBOX_EXEC);
    let system_profile = Path::new(DARWIN_SYSTEM_PROFILE);
    let dyld_profile = Path::new(DARWIN_DYLD_PROFILE);
    Ok(ExecutionEnvironmentIdentity {
        target: current_target(),
        os_version: command_value("/usr/bin/sw_vers", &["-productVersion"])?,
        os_build: command_value("/usr/bin/sw_vers", &["-buildVersion"])?,
        kernel_release: command_value("/usr/bin/uname", &["-r"])?,
        backend_policy: DARWIN_POLICY_ID.to_string(),
        backend_program: DARWIN_SANDBOX_EXEC.to_string(),
        backend_program_sha256: digest_file(backend_program)?,
        imported_system_profile: DARWIN_SYSTEM_PROFILE.to_string(),
        imported_system_profile_sha256: digest_file(system_profile)?,
        imported_dyld_profile: DARWIN_DYLD_PROFILE.to_string(),
        imported_dyld_profile_sha256: digest_file(dyld_profile)?,
        generated_profile_sha256: digest_bytes(profile.as_bytes()),
        executable_sha256: digest_file(executable)?,
    })
}

fn command_value(program: &str, args: &[&str]) -> Result<String, SandboxError> {
    let output = Command::new(program)
        .args(args)
        .env_clear()
        .output()
        .map_err(|error| SandboxError::IdentityRead {
            path: program.to_string(),
            message: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(SandboxError::IdentityRead {
            path: program.to_string(),
            message: output.status.to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn digest_file(path: &Path) -> Result<String, SandboxError> {
    let bytes = fs::read(path).map_err(|error| SandboxError::IdentityRead {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;
    Ok(digest_bytes(&bytes))
}

fn digest_bytes(bytes: &[u8]) -> String {
    let digest = sha256(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    format!("sha256:{encoded}")
}

fn sbpl_literal(path: &Path) -> Result<String, SandboxError> {
    let value = path.to_string_lossy();
    if value.contains(['"', '\\', '\n', '\r', '\0']) {
        return Err(SandboxError::InvalidExecutablePath);
    }
    Ok(value.into_owned())
}

fn wait_with_guard(child: &mut Child, wall_guard: Duration) -> Result<ExitStatus, SandboxError> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                terminate_and_reap(child);
                return Err(SandboxError::Pipe(error.to_string()));
            }
        }
        if started.elapsed() >= wall_guard {
            terminate_and_reap(child);
            return Err(SandboxError::Timeout);
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn join_writer(writer: thread::JoinHandle<Result<(), SandboxError>>) -> Result<(), SandboxError> {
    writer
        .join()
        .map_err(|_| SandboxError::Pipe("sandbox pipe writer panicked".to_string()))?
}

fn terminate_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn verify_execution_identity(
    expected: &ExecutionEnvironmentIdentity,
    executable: &Path,
    profile: &str,
) -> Result<(), SandboxError> {
    let observed = execution_identity(executable, profile)?;
    if &observed == expected {
        return Ok(());
    }
    Err(SandboxError::IdentityRead {
        path: executable.display().to_string(),
        message: "execution-environment identity changed during the sandboxed call".to_string(),
    })
}

fn read_limited(
    mut reader: impl Read,
    limit: usize,
    channel: &'static str,
) -> Result<Vec<u8>, SandboxError> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8_192];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|error| SandboxError::Pipe(error.to_string()))?;
        if read == 0 {
            return Ok(bytes);
        }
        if bytes.len().saturating_add(read) > limit {
            return Err(SandboxError::OutputLimit { channel, limit });
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

fn join_reader(
    reader: thread::JoinHandle<Result<Vec<u8>, SandboxError>>,
) -> Result<Vec<u8>, SandboxError> {
    reader
        .join()
        .map_err(|_| SandboxError::Pipe("sandbox pipe reader panicked".to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn wall_guard_covers_a_worker_that_does_not_read_its_request() {
        let mut child = Command::new("/bin/sleep")
            .arg("5")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("sleep process");
        let frame = vec![0; crate::protocol::MAX_SOURCE_BYTES + crate::protocol::MAX_INPUT_BYTES];

        assert_eq!(
            communicate_with_guard(&mut child, frame, Duration::from_millis(100)),
            Err(SandboxError::Timeout)
        );
    }

    #[cfg(unix)]
    #[test]
    fn profile_is_deny_default_and_binds_one_executable() {
        let executable = std::path::PathBuf::from("/usr/bin/true");
        let profile = DarwinSandbox::profile_for(&executable).expect("profile");
        assert!(profile.contains("(deny default)"));
        assert!(profile.contains("(deny network*)"));
        assert!(profile.contains("(deny file-write*)"));
        assert!(profile.contains("(deny process-fork)"));
        assert!(profile.contains("(literal \"/usr/bin/true\")"));
        assert!(!profile.contains("(allow network"));
    }

    #[test]
    fn unsupported_target_is_structured() {
        let target = current_target();
        if target == "aarch64-apple-darwin" {
            assert!(DarwinSandbox::qualify().is_ok());
        } else {
            assert_eq!(
                DarwinSandbox::qualify(),
                Err(SandboxError::UnsupportedTarget { target })
            );
        }
    }
}
