//! Process ABI for inherited terminal I/O and bounded metadata capture.

use std::io::{self, Read};
use std::process::{Command, Stdio};

use topaz_rt::{CallFuture, RtCx, TpzCall, Value};

use super::result_value;

const PARAMETERS: [&str; 4] = ["executable", "arguments", "directory", "environment"];
const CAPTURE_LIMIT: u64 = 1024 * 1024;

#[derive(Debug)]
pub struct Process;

impl TpzCall for Process {
    fn as_any(&self) -> &dyn std::any::Any { self }
    fn name(&self) -> Option<&str> { Some("process") }
    fn arity(&self) -> usize { PARAMETERS.len() }
    fn param_name(&self, index: usize) -> Option<&str> {
        PARAMETERS.get(index).copied()
    }
    fn call(&self, _context: RtCx, arguments: Vec<Value>) -> CallFuture {
        let result = run_process(&arguments).map(|code| Value::Int(i64::from(code)));
        Box::pin(async move { Ok(result_value(result)) })
    }
}

fn run_process(arguments: &[Value]) -> Result<i32, String> {
    let status = process_command(arguments)?.status()
        .map_err(|error| format!("cannot start process: {error}"))?;
    status.code().ok_or_else(|| "process terminated without an exit code".into())
}

fn process_command(arguments: &[Value]) -> Result<Command, String> {
    let [Value::Str(executable), Value::Array(argv), Value::Str(directory), Value::Array(environment)] = arguments else {
        return Err("process expects executable, arguments, working directory and environment".into());
    };
    let mut command = Command::new(executable.as_ref());
    command.current_dir(directory.as_ref());
    for argument in argv.borrow().iter() {
        let Value::Str(argument) = argument else {
            return Err("every process argument must be a string".into());
        };
        command.arg(argument.as_ref());
    }
    for entry in environment.borrow().iter() {
        let Value::Str(entry) = entry else {
            return Err("every process environment entry must be a KEY=value string".into());
        };
        let (key, value) = entry.split_once('=')
            .ok_or("process environment entry must contain '='")?;
        if key.is_empty() || key.contains('\0') || value.contains('\0') {
            return Err("process environment contains an empty key or NUL".into());
        }
        command.env(key, value);
    }
    Ok(command)
}

#[derive(Debug)]
pub struct Capture;

impl TpzCall for Capture {
    fn as_any(&self) -> &dyn std::any::Any { self }
    fn name(&self) -> Option<&str> { Some("capture") }
    fn arity(&self) -> usize { PARAMETERS.len() }
    fn param_name(&self, index: usize) -> Option<&str> {
        PARAMETERS.get(index).copied()
    }
    fn call(&self, _context: RtCx, arguments: Vec<Value>) -> CallFuture {
        let result = capture_process(&arguments).map(Value::str);
        Box::pin(async move { Ok(result_value(result)) })
    }
}

fn read_pipe(mut pipe: impl Read) -> io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::new();
    pipe.by_ref().take(CAPTURE_LIMIT).read_to_end(&mut output)?;
    // Drain the rest so a verbose child can finish. Both operations retry
    // interrupted reads and the discarded bytes never enter the output buffer.
    let discarded = io::copy(&mut pipe, &mut io::sink())?;
    Ok((output, discarded != 0))
}

fn capture_process(arguments: &[Value]) -> Result<String, String> {
    let mut child = process_command(arguments)?
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().map_err(|error| format!("cannot start captured process: {error}"))?;
    let stdout = child.stdout.take().ok_or("captured stdout is unavailable")?;
    let stderr = child.stderr.take().ok_or("captured stderr is unavailable")?;
    // Read the two pipes concurrently. A child can fill either pipe first.
    let stdout_reader = std::thread::spawn(move || read_pipe(stdout));
    let stderr = read_pipe(stderr);
    let status = child.wait().map_err(|error| format!("cannot wait for captured process: {error}"));
    let (stdout, stdout_overflow) = stdout_reader.join().map_err(|_| "stdout reader panicked")?
        .map_err(|error| format!("cannot capture stdout: {error}"))?;
    let (stderr, stderr_overflow) = stderr.map_err(|error| format!("cannot capture stderr: {error}"))?;
    let code = status?.code().ok_or("captured process terminated without an exit code")?;
    if stdout_overflow || stderr_overflow { return Err("captured process output exceeds 1 MiB per stream".into()); }
    let stdout = String::from_utf8(stdout).map_err(|_| "captured stdout is not UTF-8")?;
    let stderr = String::from_utf8(stderr).map_err(|_| "captured stderr is not UTF-8")?;
    topaz_rt::json_stringify(&Value::record(vec![
        ("code".into(), Value::Int(i64::from(code))),
        ("stdout".into(), Value::str(stdout)),
        ("stderr".into(), Value::str(stderr)),
    ]), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_capture_retries_interrupts_and_drains_past_the_exact_limit() {
        struct Interrupting<R> {
            reader: R,
            interrupt_next: bool,
        }
        impl<R: Read> Read for Interrupting<R> {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                self.interrupt_next = !self.interrupt_next;
                if self.interrupt_next {
                    return Err(io::ErrorKind::Interrupted.into());
                }
                self.reader.read(buffer)
            }
        }

        let limit = CAPTURE_LIMIT as usize;
        for size in [0, limit, limit + 1] {
            let input = vec![0xa5; size];
            let mut unread = input.as_slice();
            let (bytes, overflow) = read_pipe(Interrupting {
                reader: &mut unread,
                interrupt_next: false,
            }).unwrap();
            assert_eq!(bytes, input[..size.min(limit)]);
            assert_eq!(overflow, size > limit);
            assert!(unread.is_empty());
        }
    }

    #[cfg(unix)]
    #[test]
    fn capture_preserves_arguments_environment_streams_and_exit_code() {
        let captured = capture_process(&[
            Value::str("/bin/sh"),
            Value::array(vec![
                Value::str("-c"),
                Value::str(r#"printf '%s' "$1"; printf '%s' "$TOPAZ_CAPTURE_TEST" >&2; exit 19"#),
                Value::str("capture-test"),
                Value::str("two words"),
            ]),
            Value::str("."),
            Value::array(vec![Value::str("TOPAZ_CAPTURE_TEST=a=b")]),
        ]).unwrap();
        assert_eq!(captured, r#"{"code":19,"stderr":"a=b","stdout":"two words"}"#);
    }
}
