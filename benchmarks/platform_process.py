"""Process timing and machine facts at the operating-system boundary.

No workload, release-selection, statistics or reporting policy lives here.
The returned streams and measurements belong in the runner's private output.
"""

from __future__ import annotations

import ctypes
import math
import os
from pathlib import Path
import platform
import signal
import subprocess
import tempfile
import threading
import time
from typing import Any


POSIX_MEMORY_SCOPE = "process-and-waited-descendants-maximum-rss-not-sum"
WINDOWS_MEMORY_SCOPE = "root-process-peak-working-set"


def validate_windows_environment(env: dict[str, str]) -> None:
    """Reject ambiguous names before constructing a Windows environment block."""
    seen = set()
    for name in env:
        if not isinstance(name, str) or not name or "\0" in name or "=" in name:
            raise ValueError("Windows environment names must be nonempty valid strings")
        normalized = name.upper()
        if normalized in seen:
            raise ValueError("Windows environment contains case-insensitive duplicate names")
        seen.add(normalized)


def _kill_group(pid: int) -> None:
    try:
        os.killpg(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def _posix_measure(
    argv: list[str], cwd: Path, env: dict[str, str], timeout: float,
    stdout_file: Any, stderr_file: Any, memory: bool,
) -> dict[str, Any]:
    if not hasattr(os, "wait4"):
        raise RuntimeError("wait4 is unavailable on this platform")
    if memory and platform.system() not in ("Linux", "Darwin"):
        raise RuntimeError("the ru_maxrss unit is not defined for this platform")

    completed = threading.Event()
    timed_out = threading.Event()
    started = time.perf_counter_ns()
    process = subprocess.Popen(
        argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
        stdout=stdout_file, stderr=stderr_file, start_new_session=True,
    )

    def enforce_timeout() -> None:
        remaining = max(0.0, timeout - (time.perf_counter_ns() - started) / 1e9)
        if not completed.wait(remaining):
            timed_out.set()
            _kill_group(process.pid)

    watchdog = threading.Thread(target=enforce_timeout, daemon=True)
    watchdog.start()
    try:
        # A blocking kernel wait avoids polling quantization on empty programs.
        # Popen.wait/poll must not reap this PID before wait4 captures its usage.
        while True:
            try:
                _, status, usage = os.wait4(process.pid, 0)
                break
            except InterruptedError:
                continue
        stopped = time.perf_counter_ns()
        process.returncode = os.waitstatus_to_exitcode(status)
    except BaseException:
        _kill_group(process.pid)
        process.wait()
        raise
    finally:
        completed.set()
        watchdog.join()
        # A command that exits without waiting for its children must not leak
        # background work into the next trial. Cleanup is outside elapsed time.
        _kill_group(process.pid)

    peak = None
    if memory:
        multiplier = 1 if platform.system() == "Darwin" else 1024
        peak = int(usage.ru_maxrss) * multiplier
        if peak <= 0:
            raise RuntimeError("the kernel peak RSS counter is unavailable")
    return {
        "elapsed_ns": stopped - started,
        "peak_rss_bytes": peak,
        "memory_scope": POSIX_MEMORY_SCOPE if memory else "not-measured",
        "exit_code": process.returncode,
        "timed_out": timed_out.is_set(),
    }


def _windows_measure(
    argv: list[str], cwd: Path, env: dict[str, str], timeout: float,
    stdout_file: Any, stderr_file: Any, memory: bool,
) -> dict[str, Any]:
    validate_windows_environment(env)
    # Create the child suspended, put it in a kill-on-close Job, then resume it.
    # Assigning an already-running Popen child would race with its own children.
    import msvcrt
    from ctypes import wintypes

    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    psapi = ctypes.WinDLL("psapi", use_last_error=True)
    size_t = ctypes.c_size_t
    ulong_ptr = ctypes.c_size_t

    class StartupInfo(ctypes.Structure):
        _fields_ = [
            ("cb", wintypes.DWORD), ("lpReserved", wintypes.LPWSTR),
            ("lpDesktop", wintypes.LPWSTR), ("lpTitle", wintypes.LPWSTR),
            ("dwX", wintypes.DWORD), ("dwY", wintypes.DWORD),
            ("dwXSize", wintypes.DWORD), ("dwYSize", wintypes.DWORD),
            ("dwXCountChars", wintypes.DWORD), ("dwYCountChars", wintypes.DWORD),
            ("dwFillAttribute", wintypes.DWORD), ("dwFlags", wintypes.DWORD),
            ("wShowWindow", wintypes.WORD), ("cbReserved2", wintypes.WORD),
            ("lpReserved2", ctypes.POINTER(ctypes.c_ubyte)),
            ("hStdInput", wintypes.HANDLE), ("hStdOutput", wintypes.HANDLE),
            ("hStdError", wintypes.HANDLE),
        ]

    class ProcessInfo(ctypes.Structure):
        _fields_ = [("hProcess", wintypes.HANDLE), ("hThread", wintypes.HANDLE),
                    ("dwProcessId", wintypes.DWORD), ("dwThreadId", wintypes.DWORD)]

    class BasicLimit(ctypes.Structure):
        _fields_ = [
            ("PerProcessUserTimeLimit", ctypes.c_int64),
            ("PerJobUserTimeLimit", ctypes.c_int64), ("LimitFlags", wintypes.DWORD),
            ("MinimumWorkingSetSize", size_t), ("MaximumWorkingSetSize", size_t),
            ("ActiveProcessLimit", wintypes.DWORD), ("Affinity", ulong_ptr),
            ("PriorityClass", wintypes.DWORD), ("SchedulingClass", wintypes.DWORD),
        ]

    class IoCounters(ctypes.Structure):
        _fields_ = [(name, ctypes.c_uint64) for name in (
            "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
            "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]

    class ExtendedLimit(ctypes.Structure):
        _fields_ = [("BasicLimitInformation", BasicLimit),
                    ("IoInfo", IoCounters), ("ProcessMemoryLimit", size_t),
                    ("JobMemoryLimit", size_t), ("PeakProcessMemoryUsed", size_t),
                    ("PeakJobMemoryUsed", size_t)]

    class MemoryCounters(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
            (name, size_t) for name in (
                "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
                "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
                "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]

    kernel.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
    kernel.CreateJobObjectW.restype = wintypes.HANDLE
    kernel.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int,
                                               ctypes.c_void_p, wintypes.DWORD]
    kernel.SetInformationJobObject.restype = wintypes.BOOL
    kernel.CreateProcessW.argtypes = [
        wintypes.LPCWSTR, wintypes.LPWSTR, ctypes.c_void_p, ctypes.c_void_p,
        wintypes.BOOL, wintypes.DWORD, ctypes.c_void_p, wintypes.LPCWSTR,
        ctypes.POINTER(StartupInfo), ctypes.POINTER(ProcessInfo),
    ]
    kernel.CreateProcessW.restype = wintypes.BOOL
    kernel.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
    kernel.AssignProcessToJobObject.restype = wintypes.BOOL
    kernel.ResumeThread.argtypes = [wintypes.HANDLE]
    kernel.ResumeThread.restype = wintypes.DWORD
    kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel.WaitForSingleObject.restype = wintypes.DWORD
    kernel.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
    kernel.GetExitCodeProcess.restype = wintypes.BOOL
    kernel.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
    kernel.TerminateJobObject.restype = wintypes.BOOL
    kernel.TerminateProcess.argtypes = [wintypes.HANDLE, wintypes.UINT]
    kernel.TerminateProcess.restype = wintypes.BOOL
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.CloseHandle.restype = wintypes.BOOL
    psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE,
                                          ctypes.POINTER(MemoryCounters), wintypes.DWORD]
    psapi.GetProcessMemoryInfo.restype = wintypes.BOOL

    def require(ok: Any, operation: str) -> None:
        if not ok:
            raise OSError(ctypes.get_last_error(), operation)

    job = kernel.CreateJobObjectW(None, None)
    require(job, "CreateJobObjectW")
    process = ProcessInfo()
    try:
        limits = ExtendedLimit()
        limits.BasicLimitInformation.LimitFlags = 0x00002000  # KILL_ON_JOB_CLOSE
        require(kernel.SetInformationJobObject(job, 9, ctypes.byref(limits),
                                               ctypes.sizeof(limits)),
                "SetInformationJobObject")
        with open(os.devnull, "rb") as stdin_file:
            handles = [msvcrt.get_osfhandle(f.fileno())
                       for f in (stdin_file, stdout_file, stderr_file)]
            for handle in handles:
                os.set_handle_inheritable(handle, True)
            try:
                startup = StartupInfo()
                startup.cb = ctypes.sizeof(startup)
                startup.dwFlags = 0x00000100  # STARTF_USESTDHANDLES
                startup.hStdInput, startup.hStdOutput, startup.hStdError = handles
                command = ctypes.create_unicode_buffer(subprocess.list2cmdline(argv))
                environment = ctypes.create_unicode_buffer(
                    "\0".join(f"{key}={value}" for key, value in
                              sorted(env.items(), key=lambda item: item[0].upper())) + "\0\0"
                )
                started = time.perf_counter_ns()
                require(kernel.CreateProcessW(
                    None, command, None, None, True,
                    0x00000004 | 0x00000400 | 0x08000000,
                    environment, str(cwd), ctypes.byref(startup), ctypes.byref(process)),
                    "CreateProcessW")
            finally:
                for handle in handles:
                    os.set_handle_inheritable(handle, False)
        require(kernel.AssignProcessToJobObject(job, process.hProcess),
                "AssignProcessToJobObject; descendant cleanup unavailable")
        if kernel.ResumeThread(process.hThread) == 0xFFFFFFFF:
            require(False, "ResumeThread")
        remaining = max(0.0, timeout - (time.perf_counter_ns() - started) / 1e9)
        wait_status = kernel.WaitForSingleObject(
            process.hProcess, min(0xFFFFFFFE, math.ceil(remaining * 1000)))
        timed_out = wait_status == 0x00000102  # WAIT_TIMEOUT
        if timed_out:
            require(kernel.TerminateJobObject(job, 1), "TerminateJobObject")
            wait_status = kernel.WaitForSingleObject(process.hProcess, 0xFFFFFFFF)
        if wait_status != 0:
            raise OSError(ctypes.get_last_error(), "WaitForSingleObject")
        stopped = time.perf_counter_ns()
        exit_code = wintypes.DWORD()
        require(kernel.GetExitCodeProcess(process.hProcess, ctypes.byref(exit_code)),
                "GetExitCodeProcess")
        peak = None
        if memory:
            counters = MemoryCounters()
            counters.cb = ctypes.sizeof(counters)
            require(psapi.GetProcessMemoryInfo(process.hProcess, ctypes.byref(counters),
                                               ctypes.sizeof(counters)),
                    "GetProcessMemoryInfo; kernel peak working set unavailable")
            peak = int(counters.PeakWorkingSetSize)
            if peak <= 0:
                raise RuntimeError("the kernel peak working set counter is unavailable")
        return {
            "elapsed_ns": stopped - started,
            "peak_rss_bytes": peak,
            "memory_scope": WINDOWS_MEMORY_SCOPE if memory else "not-measured",
            "exit_code": int(exit_code.value), "timed_out": timed_out,
        }
    finally:
        if process.hProcess:
            # This also handles failure before the suspended child joined its Job.
            kernel.TerminateProcess(process.hProcess, 1)
            kernel.WaitForSingleObject(process.hProcess, 0xFFFFFFFF)
        kernel.CloseHandle(job)
        if process.hThread:
            kernel.CloseHandle(process.hThread)
        if process.hProcess:
            kernel.CloseHandle(process.hProcess)


def measure(
    argv: list[str], cwd: Path, env: dict[str, str], timeout: float,
    capture_dir: Path, memory: bool = True,
) -> dict[str, Any]:
    """Run one command, returning private streams and OS measurements.

    Wall time includes process creation through kernel-reported termination.
    Linux ru_maxrss is converted from KiB; Darwin already reports bytes. POSIX
    wait4 accounts for the process and waited descendants as a maximum, not a
    sum. Windows returns the root process's kernel PeakWorkingSetSize. These
    scopes must stay labelled and must not be pooled across operating systems.
    Builds pass memory=False because build trees have different process shapes.
    A missing high-water counter raises an error; it is never estimated by RSS
    sampling or substituted with the measurement runner's memory.
    """
    if not argv or any(not isinstance(arg, str) or "\0" in arg for arg in argv):
        raise ValueError("argv must be a nonempty list of strings without NUL")
    if not math.isfinite(timeout) or timeout <= 0:
        raise ValueError("timeout must be finite and positive")
    if any(not isinstance(key, str) or not isinstance(value, str)
           or "\0" in key or "\0" in value or "=" in key
           for key, value in env.items()):
        raise ValueError("environment entries must be valid string pairs")
    cwd = Path(cwd).resolve(strict=True)
    capture_dir = Path(capture_dir)
    capture_dir.mkdir(parents=True, exist_ok=True)
    implementation = _windows_measure if os.name == "nt" else _posix_measure
    with tempfile.TemporaryFile(dir=capture_dir) as stdout_file, \
            tempfile.TemporaryFile(dir=capture_dir) as stderr_file:
        result = implementation(argv, cwd, env, timeout, stdout_file, stderr_file, memory)
        stdout_file.seek(0)
        stderr_file.seek(0)
        result["stdout"] = stdout_file.read()
        result["stderr"] = stderr_file.read()
        return result


def capture(
    argv: list[str], cwd: Path, env: dict[str, str], timeout: float,
    capture_dir: Path,
) -> dict[str, Any]:
    """Capture an untimed command with the same timeout and tree cleanup.

    The clock still enforces the timeout. No timing or memory field is retained
    in the result used by setup, correctness checks or quick-mode runs.
    """
    result = measure(argv, cwd, env, timeout, capture_dir, memory=False)
    return {key: result[key] for key in ("exit_code", "timed_out", "stdout", "stderr")}


def _sysctl(name: str) -> str | None:
    try:
        return subprocess.check_output(["/usr/sbin/sysctl", "-n", name],
                                       text=True, timeout=5).strip() or None
    except (OSError, subprocess.SubprocessError):
        return None


def machine_metadata() -> dict[str, Any]:
    """Capture machine facts without inventing values when a source is absent."""
    facts: dict[str, Any] = {
        "os": platform.system(), "os_release": platform.release(),
        "os_version": platform.version(), "architecture": platform.machine(),
        "cpu_model": None, "logical_cpus": os.cpu_count(),
        "physical_cpu_cores": None, "physical_memory_bytes": None,
        "python_version": platform.python_version(),
    }
    system = platform.system()
    if system == "Linux":
        try:
            cpuinfo = Path("/proc/cpuinfo").read_text(encoding="utf-8")
            processors = [dict(line.split(":", 1) for line in block.splitlines()
                               if ":" in line) for block in cpuinfo.strip().split("\n\n")]
            processors = [{k.strip(): v.strip() for k, v in p.items()} for p in processors]
            facts["cpu_model"] = next((p.get("model name") or p.get("Processor")
                                       for p in processors
                                       if p.get("model name") or p.get("Processor")), None)
            cores = {(p["physical id"], p["core id"]) for p in processors
                     if "physical id" in p and "core id" in p}
            facts["physical_cpu_cores"] = len(cores) if cores else None
        except (OSError, UnicodeError):
            pass
        try:
            facts["physical_memory_bytes"] = os.sysconf("SC_PHYS_PAGES") * os.sysconf("SC_PAGE_SIZE")
        except (OSError, ValueError):
            pass
        try:
            facts["cpu_affinity"] = sorted(os.sched_getaffinity(0))
        except (AttributeError, OSError):
            facts["cpu_affinity"] = None
        try:
            facts["os_distribution"] = dict(platform.freedesktop_os_release())
        except (AttributeError, OSError):
            facts["os_distribution"] = None
    elif system == "Darwin":
        facts["cpu_model"] = _sysctl("machdep.cpu.brand_string") or _sysctl("hw.model")
        for name, query in (("physical_cpu_cores", "hw.physicalcpu"),
                            ("physical_memory_bytes", "hw.memsize")):
            value = _sysctl(query)
            if value and value.isdecimal():
                facts[name] = int(value)
    elif system == "Windows":
        import winreg
        from ctypes import wintypes
        try:
            with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,
                                r"HARDWARE\DESCRIPTION\System\CentralProcessor\0") as key:
                facts["cpu_model"] = winreg.QueryValueEx(key, "ProcessorNameString")[0].strip()
        except OSError:
            pass

        class MemoryStatus(ctypes.Structure):
            _fields_ = [("dwLength", wintypes.DWORD), ("dwMemoryLoad", wintypes.DWORD)] + [
                (name, ctypes.c_uint64) for name in (
                    "ullTotalPhys", "ullAvailPhys", "ullTotalPageFile", "ullAvailPageFile",
                    "ullTotalVirtual", "ullAvailVirtual", "ullAvailExtendedVirtual")]

        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        status = MemoryStatus()
        status.dwLength = ctypes.sizeof(status)
        kernel.GlobalMemoryStatusEx.argtypes = [ctypes.POINTER(MemoryStatus)]
        kernel.GlobalMemoryStatusEx.restype = wintypes.BOOL
        if kernel.GlobalMemoryStatusEx(ctypes.byref(status)):
            facts["physical_memory_bytes"] = int(status.ullTotalPhys)
        kernel.GetLogicalProcessorInformationEx.argtypes = [
            ctypes.c_int, ctypes.c_void_p, ctypes.POINTER(wintypes.DWORD)]
        kernel.GetLogicalProcessorInformationEx.restype = wintypes.BOOL
        length = wintypes.DWORD()
        kernel.GetLogicalProcessorInformationEx(0, None, ctypes.byref(length))
        if length.value:
            buffer = ctypes.create_string_buffer(length.value)
            if kernel.GetLogicalProcessorInformationEx(0, buffer, ctypes.byref(length)):
                offset, cores = 0, 0
                while offset + 8 <= length.value:
                    record_size = wintypes.DWORD.from_buffer(buffer, offset + 4).value
                    if record_size < 8 or offset + record_size > length.value:
                        cores = 0
                        break
                    cores += 1
                    offset += record_size
                facts["physical_cpu_cores"] = cores or None
    clock = time.get_clock_info("perf_counter")
    facts["measurement_clock"] = {
        "name": "perf_counter_ns", "implementation": clock.implementation,
        "monotonic": clock.monotonic, "adjustable": clock.adjustable,
        "resolution_seconds": clock.resolution,
    }
    facts["unavailable"] = sorted(key for key, value in facts.items() if value is None)
    return facts
