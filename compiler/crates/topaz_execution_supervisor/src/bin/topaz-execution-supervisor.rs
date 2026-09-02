#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::thread;

#[cfg(unix)]
use topaz_execution_sandbox::protocol::{WorkerRequest, WorkerStatus};
#[cfg(unix)]
use topaz_execution_supervisor::{
    ExecutionSupervisor, OperationalOutcome, ResourceLimits, SupervisedRun, default_capture_limit,
    process_is_alive,
};

#[cfg(not(unix))]
fn main() {
    eprintln!("topaz-execution-supervisor is unsupported on non-Unix platforms");
    std::process::exit(1);
}

#[cfg(unix)]
fn main() {
    let mut args = std::env::args_os();
    let _program = args.next();
    let Some(bundle) = args.next().map(PathBuf::from) else {
        eprintln!("private execution supervisor requires one bundle directory");
        std::process::exit(64);
    };
    if args.next().is_some() {
        eprintln!("private execution supervisor accepts one bundle directory");
        std::process::exit(64);
    }
    match run_supervision(&bundle) {
        Ok(summary) => println!("{summary}"),
        Err(error) => {
            eprintln!("execution supervision failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(unix)]
fn run_supervision(bundle: &Path) -> Result<String, String> {
    let worker = bundle.join("bin/topaz-private-worker");
    let probe = bundle.join("bin/topaz-sandbox-probe");
    let invalid = bundle.join("policy/execution-sandbox-policy.json");
    for path in [&worker, &probe, &invalid] {
        if !path.exists() {
            return Err(format!("bundle input is missing: {}", path.display()));
        }
    }

    let supervisor = ExecutionSupervisor;
    let ordinary = ResourceLimits::new(5_000, 2, 524_288, default_capture_limit());
    let mut completed_pids = Vec::new();

    let pure = supervisor
        .run(&worker, &request("let 값 = 40 + 2\n값", ""), ordinary)
        .map_err(|error| error.to_string())?;
    expect_completed(&pure, WorkerStatus::Success, "pure success")?;
    completed_pids.push(pure.worker_pid);

    let static_rejection = supervisor
        .run(
            &worker,
            &request("let 값: int = \"잘못됨\"\n값", ""),
            ordinary,
        )
        .map_err(|error| error.to_string())?;
    expect_completed(
        &static_rejection,
        WorkerStatus::StaticRejected,
        "static rejection",
    )?;
    completed_pids.push(static_rejection.worker_pid);

    let denied = supervisor
        .run(
            &worker,
            &request("FS.readText(\"secret.txt\")", ""),
            ordinary,
        )
        .map_err(|error| error.to_string())?;
    expect_completed(&denied, WorkerStatus::Success, "host denial")?;
    completed_pids.push(denied.worker_pid);

    let wall = supervisor
        .run(
            &worker,
            &request("let mut i = 0\nwhile true { i += 1 }\ni", ""),
            ResourceLimits::new(100, 0, 524_288, default_capture_limit()),
        )
        .map_err(|error| error.to_string())?;
    expect_operational(&wall, "wall limit", |outcome| {
        matches!(outcome, OperationalOutcome::WallLimit)
    })?;
    completed_pids.push(wall.worker_pid);

    let cpu = supervisor
        .run(
            &worker,
            &request("let mut i = 0\nwhile true { i += 1 }\ni", ""),
            ResourceLimits::new(5_000, 1, 524_288, default_capture_limit()),
        )
        .map_err(|error| error.to_string())?;
    expect_operational(&cpu, "CPU limit", |outcome| {
        matches!(outcome, OperationalOutcome::CpuLimit)
    })?;
    completed_pids.push(cpu.worker_pid);

    let memory = supervisor
        .run(
            &worker,
            &request(
                concat!(
                    "let mut 값들: Array<string> = []\n",
                    "let mut 번호 = 0\n",
                    "while true {\n",
                    "    값들.push(\"{번호}-0123456789012345678901234567890123456789\")\n",
                    "    번호 += 1\n",
                    "}\n",
                    "번호"
                ),
                "",
            ),
            ResourceLimits::new(5_000, 0, 8_192, default_capture_limit()),
        )
        .map_err(|error| error.to_string())?;
    expect_operational(&memory, "memory limit", |outcome| {
        matches!(outcome, OperationalOutcome::MemoryLimit { .. })
    })?;
    completed_pids.push(memory.worker_pid);

    let output = supervisor
        .run(
            &worker,
            &request(
                concat!(
                    "let mut 번호 = 0\n",
                    "while 번호 < 30000 {\n",
                    "    print(\"0123456789\")\n",
                    "    번호 += 1\n",
                    "}\n",
                    "번호"
                ),
                "",
            ),
            ResourceLimits::new(5_000, 2, 524_288, default_capture_limit()),
        )
        .map_err(|error| error.to_string())?;
    expect_completed(&output, WorkerStatus::HostLimit, "output limit")?;
    completed_pids.push(output.worker_pid);

    let sandbox_failure = supervisor
        .run(&invalid, &request("1", ""), ordinary)
        .map_err(|error| error.to_string())?;
    expect_operational(&sandbox_failure, "sandbox failure", |outcome| {
        matches!(outcome, OperationalOutcome::SandboxFailure { .. })
    })?;
    completed_pids.push(sandbox_failure.worker_pid);

    let worker_fault = supervisor
        .run(&probe, &request("1", ""), ordinary)
        .map_err(|error| error.to_string())?;
    expect_operational(&worker_fault, "worker fault", |outcome| {
        matches!(outcome, OperationalOutcome::WorkerFault { .. })
    })?;
    completed_pids.push(worker_fault.worker_pid);

    let first = supervisor
        .run(&worker, &request("let 숨은값 = 7\n숨은값", ""), ordinary)
        .map_err(|error| error.to_string())?;
    expect_completed(&first, WorkerStatus::Success, "isolation first call")?;
    let second = supervisor
        .run(&worker, &request("숨은값", ""), ordinary)
        .map_err(|error| error.to_string())?;
    expect_completed(
        &second,
        WorkerStatus::StaticRejected,
        "isolation second call",
    )?;
    completed_pids.extend([first.worker_pid, second.worker_pid]);

    let worker_a = worker.clone();
    let worker_b = worker.clone();
    let overlap_a = thread::spawn(move || {
        ExecutionSupervisor.run(&worker_a, &request("\"첫째\"", "입력A"), ordinary)
    });
    let overlap_b = thread::spawn(move || {
        ExecutionSupervisor.run(&worker_b, &request("\"둘째\"", "입력B"), ordinary)
    });
    let overlap_a = overlap_a
        .join()
        .map_err(|_| "overlap A panicked".to_string())?
        .map_err(|error| error.to_string())?;
    let overlap_b = overlap_b
        .join()
        .map_err(|_| "overlap B panicked".to_string())?
        .map_err(|error| error.to_string())?;
    expect_value(&overlap_a, "첫째", "overlap A")?;
    expect_value(&overlap_b, "둘째", "overlap B")?;
    completed_pids.extend([overlap_a.worker_pid, overlap_b.worker_pid]);

    let marker = format!("TOPAZ_EXECUTION_MARKER_{}", std::process::id());
    let marker_source = format!("let {marker} = 41\n{marker} + 1");
    let marker_run = supervisor
        .run(&worker, &request(&marker_source, ""), ordinary)
        .map_err(|error| error.to_string())?;
    expect_value(&marker_run, "42", "marker run")?;
    if marker_run.source_in_arguments
        || marker_run.source_in_environment
        || marker_run.captured_stderr.contains(&marker)
    {
        return Err("source marker escaped into process metadata or stderr".to_string());
    }
    if tree_contains(bundle, marker.as_bytes())? {
        return Err("source marker persisted in the installed bundle".to_string());
    }
    completed_pids.push(marker_run.worker_pid);

    for pid in &completed_pids {
        if process_is_alive(*pid) {
            return Err(format!("worker PID {pid} remained live after its call"));
        }
    }

    Ok(concat!(
        "{\"schema\":\"topaz.execution-supervisor-result/v1\",",
        "\"status\":\"passed\",\"cases\":13,",
        "\"resourceOutcomes\":[\"wall\",\"cpu\",\"memory\",\"output\"],",
        "\"nonretention\":true,\"sequentialIsolation\":true,",
        "\"overlapIsolation\":true,\"allWorkersReaped\":true,",
        "\"topazRun\":false}"
    )
    .to_string())
}

#[cfg(unix)]
fn request(source: &str, input: &str) -> WorkerRequest {
    WorkerRequest {
        source: source.to_string(),
        input: input.to_string(),
    }
}

#[cfg(unix)]
fn expect_completed(run: &SupervisedRun, status: WorkerStatus, label: &str) -> Result<(), String> {
    match &run.outcome {
        OperationalOutcome::Completed(response) if response.status == status => Ok(()),
        outcome => Err(format!("{label} returned {outcome:?}")),
    }
}

#[cfg(unix)]
fn expect_value(run: &SupervisedRun, value: &str, label: &str) -> Result<(), String> {
    match &run.outcome {
        OperationalOutcome::Completed(response)
            if response.status == WorkerStatus::Success && response.value == value =>
        {
            Ok(())
        }
        outcome => Err(format!("{label} returned {outcome:?}")),
    }
}

#[cfg(unix)]
fn expect_operational(
    run: &SupervisedRun,
    label: &str,
    predicate: impl FnOnce(&OperationalOutcome) -> bool,
) -> Result<(), String> {
    if predicate(&run.outcome) {
        Ok(())
    } else {
        Err(format!("{label} returned {:?}", run.outcome))
    }
}

#[cfg(unix)]
fn tree_contains(root: &Path, marker: &[u8]) -> Result<bool, String> {
    for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            if tree_contains(&path, marker)? {
                return Ok(true);
            }
        } else if fs::read(&path)
            .map_err(|error| error.to_string())?
            .windows(marker.len())
            .any(|window| window == marker)
        {
            return Ok(true);
        }
    }
    Ok(false)
}
