//! Resource and isolation supervision for installed bounded execution.
//!
//! This crate adds no product command. The packaged supervisor records
//! operational outcomes separately from the Topaz worker response.

#![cfg(unix)]

use std::fmt;
use std::io::{Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use topaz_execution_sandbox::protocol::{
    MAX_RESPONSE_BYTES, ProtocolError, WorkerRequest, WorkerResponse,
};
use topaz_execution_sandbox::sandbox::DarwinSandbox;

const LAUNCHER: &str = "/bin/sh";
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
const PS: &str = "/bin/ps";
// Darwin reports CPU-time exhaustion as SIGXCPU (signal 24).
const CPU_LIMIT_SIGNAL: i32 = 24;
const RESOURCE_LAUNCH_SCRIPT: &str = concat!(
    "ulimit -c 0 || exit 125; ",
    "if [ \"$1\" != 0 ]; then ulimit -t \"$1\" || exit 125; fi; ",
    "exec /usr/bin/sandbox-exec -p \"$2\" -- \"$3\""
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    pub wall_millis: u64,
    pub cpu_seconds: u64,
    pub rss_kib: u64,
    pub capture_bytes: usize,
}

impl ResourceLimits {
    pub const fn new(
        wall_millis: u64,
        cpu_seconds: u64,
        rss_kib: u64,
        capture_bytes: usize,
    ) -> Self {
        Self {
            wall_millis,
            cpu_seconds,
            rss_kib,
            capture_bytes,
        }
    }

    fn validate(self) -> Result<Self, SupervisorError> {
        if self.wall_millis == 0 || self.capture_bytes == 0 {
            return Err(SupervisorError::InvalidLimits);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationalOutcome {
    Completed(WorkerResponse),
    WallLimit,
    CpuLimit,
    MemoryLimit { observed_kib: u64, limit_kib: u64 },
    OutputLimit { channel: &'static str },
    SandboxFailure { status: String },
    WorkerFault { status: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisedRun {
    pub worker_pid: u32,
    pub outcome: OperationalOutcome,
    pub source_in_arguments: bool,
    pub source_in_environment: bool,
    pub captured_stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorError {
    InvalidLimits,
    SandboxProfile(String),
    Spawn(String),
    Pipe(String),
    Protocol(String),
}

impl fmt::Display for SupervisorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits => f.write_str("execution supervisor limits are invalid"),
            Self::SandboxProfile(message) => write!(f, "sandbox profile failed: {message}"),
            Self::Spawn(message) => write!(f, "supervised worker spawn failed: {message}"),
            Self::Pipe(message) => write!(f, "supervised worker pipe failed: {message}"),
            Self::Protocol(message) => write!(f, "supervised worker protocol failed: {message}"),
        }
    }
}

impl std::error::Error for SupervisorError {}

impl From<ProtocolError> for SupervisorError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value.to_string())
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionSupervisor;

impl ExecutionSupervisor {
    pub fn run(
        &self,
        executable: &Path,
        request: &WorkerRequest,
        limits: ResourceLimits,
    ) -> Result<SupervisedRun, SupervisorError> {
        let limits = limits.validate()?;
        let executable = executable
            .canonicalize()
            .map_err(|error| SupervisorError::SandboxProfile(error.to_string()))?;
        let profile = DarwinSandbox::profile_for(&executable)
            .map_err(|error| SupervisorError::SandboxProfile(error.to_string()))?;
        let frame = request.encode()?;
        let source_in_arguments = [
            RESOURCE_LAUNCH_SCRIPT,
            &limits.cpu_seconds.to_string(),
            &profile,
            &executable.display().to_string(),
        ]
        .iter()
        .any(|argument| argument.contains(&request.source));

        let mut child = Command::new(LAUNCHER)
            .args([
                "-c",
                RESOURCE_LAUNCH_SCRIPT,
                "topaz-execution-supervisor",
                &limits.cpu_seconds.to_string(),
                &profile,
            ])
            .arg(&executable)
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| SupervisorError::Spawn(error.to_string()))?;
        let worker_pid = child.id();
        let source_in_environment = false;

        let communication = communicate_with_limits(&mut child, frame, limits)?;
        let captured_stderr = String::from_utf8_lossy(&communication.stderr).to_string();

        let outcome = match communication.outcome {
            MonitorOutcome::WallLimit => OperationalOutcome::WallLimit,
            MonitorOutcome::MemoryLimit { observed_kib } => OperationalOutcome::MemoryLimit {
                observed_kib,
                limit_kib: limits.rss_kib,
            },
            MonitorOutcome::Exited(status) => {
                if communication.stdout_exceeded {
                    OperationalOutcome::OutputLimit { channel: "stdout" }
                } else if communication.stderr_exceeded {
                    OperationalOutcome::OutputLimit { channel: "stderr" }
                } else if status.signal() == Some(CPU_LIMIT_SIGNAL) {
                    OperationalOutcome::CpuLimit
                } else if !status.success() {
                    let status = status.to_string();
                    if captured_stderr.contains("sandbox-exec")
                        || captured_stderr.contains("ulimit")
                    {
                        OperationalOutcome::SandboxFailure { status }
                    } else {
                        OperationalOutcome::WorkerFault { status }
                    }
                } else {
                    OperationalOutcome::Completed(WorkerResponse::read_from(
                        &mut communication.stdout.as_slice(),
                    )?)
                }
            }
        };

        Ok(SupervisedRun {
            worker_pid,
            outcome,
            source_in_arguments,
            source_in_environment,
            captured_stderr,
        })
    }
}

enum MonitorOutcome {
    Exited(ExitStatus),
    WallLimit,
    MemoryLimit { observed_kib: u64 },
}

struct Communication {
    outcome: MonitorOutcome,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_exceeded: bool,
    stderr_exceeded: bool,
}

fn communicate_with_limits(
    child: &mut Child,
    frame: Vec<u8>,
    limits: ResourceLimits,
) -> Result<Communication, SupervisorError> {
    let Some(mut stdin) = child.stdin.take() else {
        terminate_and_reap(child);
        return Err(SupervisorError::Pipe(
            "worker stdin unavailable".to_string(),
        ));
    };
    let Some(stdout) = child.stdout.take() else {
        terminate_and_reap(child);
        return Err(SupervisorError::Pipe(
            "worker stdout unavailable".to_string(),
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        terminate_and_reap(child);
        return Err(SupervisorError::Pipe(
            "worker stderr unavailable".to_string(),
        ));
    };
    let stdout_exceeded = Arc::new(AtomicBool::new(false));
    let stderr_exceeded = Arc::new(AtomicBool::new(false));
    let stdin_writer = thread::spawn(move || {
        stdin
            .write_all(&frame)
            .map_err(|error| SupervisorError::Pipe(error.to_string()))
    });
    let stdout_reader = spawn_reader(stdout, limits.capture_bytes, Arc::clone(&stdout_exceeded));
    let stderr_reader = spawn_reader(stderr, limits.capture_bytes, Arc::clone(&stderr_exceeded));

    let outcome = monitor(child, limits);
    let stdin = join_writer(stdin_writer);
    let stdout = join_reader(stdout_reader);
    let stderr = join_reader(stderr_reader);
    let outcome = outcome?;
    if matches!(outcome, MonitorOutcome::Exited(_)) {
        stdin?;
    }
    Ok(Communication {
        outcome,
        stdout: stdout?,
        stderr: stderr?,
        stdout_exceeded: stdout_exceeded.load(Ordering::Relaxed),
        stderr_exceeded: stderr_exceeded.load(Ordering::Relaxed),
    })
}

fn monitor(child: &mut Child, limits: ResourceLimits) -> Result<MonitorOutcome, SupervisorError> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(MonitorOutcome::Exited(status)),
            Ok(None) => {}
            Err(error) => {
                terminate_and_reap(child);
                return Err(SupervisorError::Pipe(error.to_string()));
            }
        }
        if limits.rss_kib > 0
            && let Some(observed_kib) = rss_kib(child.id())
            && observed_kib > limits.rss_kib
        {
            terminate_and_reap(child);
            return Ok(MonitorOutcome::MemoryLimit { observed_kib });
        }
        if started.elapsed() >= Duration::from_millis(limits.wall_millis) {
            terminate_and_reap(child);
            return Ok(MonitorOutcome::WallLimit);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn rss_kib(pid: u32) -> Option<u64> {
    let output = Command::new(PS)
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .env_clear()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn spawn_reader(
    reader: impl Read + Send + 'static,
    limit: usize,
    exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<Result<Vec<u8>, SupervisorError>> {
    thread::spawn(move || read_limited(reader, limit, exceeded))
}

fn read_limited(
    mut reader: impl Read,
    limit: usize,
    exceeded: Arc<AtomicBool>,
) -> Result<Vec<u8>, SupervisorError> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8_192];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|error| SupervisorError::Pipe(error.to_string()))?;
        if read == 0 {
            return Ok(bytes);
        }
        let remaining = limit.saturating_sub(bytes.len());
        if read > remaining {
            bytes.extend_from_slice(&chunk[..remaining]);
            exceeded.store(true, Ordering::Relaxed);
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

fn join_reader(
    reader: thread::JoinHandle<Result<Vec<u8>, SupervisorError>>,
) -> Result<Vec<u8>, SupervisorError> {
    reader
        .join()
        .map_err(|_| SupervisorError::Pipe("supervisor pipe reader panicked".to_string()))?
}

fn join_writer(
    writer: thread::JoinHandle<Result<(), SupervisorError>>,
) -> Result<(), SupervisorError> {
    writer
        .join()
        .map_err(|_| SupervisorError::Pipe("supervisor pipe writer panicked".to_string()))?
}

fn terminate_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

pub fn process_is_alive(pid: u32) -> bool {
    Command::new(PS)
        .args(["-p", &pid.to_string()])
        .env_clear()
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub fn launcher_inputs() -> [&'static str; 3] {
    [LAUNCHER, SANDBOX_EXEC, PS]
}

pub fn default_capture_limit() -> usize {
    MAX_RESPONSE_BYTES + 4_096
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn wall_limit_covers_a_worker_that_does_not_read_its_request() {
        let mut child = Command::new("/bin/sleep")
            .arg("5")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("sleep process");
        let frame = vec![
            0;
            topaz_execution_sandbox::protocol::MAX_SOURCE_BYTES
                + topaz_execution_sandbox::protocol::MAX_INPUT_BYTES
        ];

        let communication = communicate_with_limits(
            &mut child,
            frame,
            ResourceLimits::new(100, 0, 0, default_capture_limit()),
        )
        .expect("wall-limited communication");
        assert!(matches!(communication.outcome, MonitorOutcome::WallLimit));
    }
}
