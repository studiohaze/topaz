//! The effect boundary (CDR-003 §1): the evaluator core never
//! touches `std::fs`/`std::io`/clocks/threads — every observable
//! effect crosses this trait, which is what keeps the core
//! WASM-compatible and the execution corpus deterministic.

use std::cell::RefCell;
use std::collections::BTreeMap;

// The `Host` effect trait and the opaque `ResourceId` handle live in the shared
// core (CDR-006 §3): they are the bottom-layer ABI the callables compile
// against. The NATIVE std-backed host moved to its own leaf crate
// (`topaz_host_native`) so emitted binaries need not pull in the interpreter;
// the corpus `TestHost` (below) is the implementation that stays here.
pub use topaz_value::{ExternReplayStore, Host, HostDirEntry, ResourceId, Value};

/// Corpus/test host: captured transcript, virtual files, and a
/// virtual clock the test advances explicitly — the determinism the
/// differential gate relies on (CDR-003 §1, §11).
#[derive(Debug, Default)]
pub struct TestHost {
    state: RefCell<TestState>,
}

#[derive(Debug, Default)]
struct TestState {
    stdout: Vec<String>,
    defer_errors: Vec<String>,
    files: BTreeMap<String, Vec<u8>>,
    open: BTreeMap<u64, (String, bool)>,
    next_handle: u64,
    now: u64,
    /// Auto-advance per `now_millis` call: lets timeout fixtures
    /// progress the clock deterministically without a real clock.
    tick_per_poll: u64,
    /// §22 the host-provided `input()` payload (default `""`).
    input: String,
    /// v5.4 manifest extern replay rows.
    extern_replay: ExternReplayStore,
}

impl TestHost {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_file(&self, path: impl Into<String>, contents: impl Into<String>) {
        self.state
            .borrow_mut()
            .files
            .insert(path.into(), contents.into().into_bytes());
    }

    /// Advance the virtual clock.
    pub fn advance_millis(&self, ms: u64) {
        self.state.borrow_mut().now += ms;
    }

    /// Every `now_millis` poll advances the clock by `ms` (0 = frozen
    /// clock, the default).
    pub fn set_tick_per_poll(&self, ms: u64) {
        self.state.borrow_mut().tick_per_poll = ms;
    }

    /// §22 set the `input()` payload this run observes.
    pub fn set_input(&self, s: impl Into<String>) {
        self.state.borrow_mut().input = s.into();
    }

    pub fn set_extern_replay(&self, replay: ExternReplayStore) {
        self.state.borrow_mut().extern_replay = replay;
    }

    pub fn stdout(&self) -> Vec<String> {
        self.state.borrow().stdout.clone()
    }

    pub fn defer_errors(&self) -> Vec<String> {
        self.state.borrow().defer_errors.clone()
    }

    /// Final virtual-file state (corpus transcript channel).
    pub fn files(&self) -> BTreeMap<String, String> {
        self.state
            .borrow()
            .files
            .iter()
            .map(|(path, bytes)| (path.clone(), String::from_utf8_lossy(bytes).into_owned()))
            .collect()
    }
}

impl Host for TestHost {
    fn print(&self, line: &str) {
        self.state.borrow_mut().stdout.push(line.to_string());
    }

    fn open(&self, path: &str) -> Result<ResourceId, String> {
        let mut state = self.state.borrow_mut();
        if !state.files.contains_key(path) {
            return Err(format!("cannot open `{path}`: not found"));
        }
        state.next_handle += 1;
        let id = state.next_handle;
        state.open.insert(id, (path.to_string(), false));
        Ok(ResourceId(id))
    }

    fn read(&self, handle: ResourceId) -> Result<String, String> {
        let state = self.state.borrow();
        let (path, closed) = state.open.get(&handle.0).ok_or("file is not open")?;
        if *closed {
            return Err("file is closed".to_string());
        }
        state
            .files
            .get(path)
            .cloned()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or_else(|| format!("cannot read `{path}`"))
    }

    fn write(&self, handle: ResourceId, s: &str) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        let (path, closed) = state
            .open
            .get(&handle.0)
            .cloned()
            .ok_or("file is not open")?;
        if closed {
            return Err("file is closed".to_string());
        }
        state.files.insert(path, s.as_bytes().to_vec());
        Ok(())
    }

    fn close(&self, handle: ResourceId) {
        if let Some(entry) = self.state.borrow_mut().open.get_mut(&handle.0) {
            entry.1 = true;
        }
    }

    fn lispex_application(
        &self,
        _request: topaz_value::LispexApplicationRequest,
    ) -> topaz_value::LispexApplicationResponse {
        topaz_value::LispexApplicationResponse::OperationalFault {
            code: "target-unavailable".into(),
            detail: None,
        }
    }

    fn read_bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        self.state
            .borrow()
            .files
            .get(path)
            .cloned()
            .ok_or_else(|| format!("cannot read `{path}`"))
    }

    fn write_bytes(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.state
            .borrow_mut()
            .files
            .insert(path.to_string(), bytes.to_vec());
        Ok(())
    }

    fn list_dir(&self, path: &str) -> Result<Vec<HostDirEntry>, String> {
        let prefix = match path {
            "" | "." => String::new(),
            p if p.ends_with('/') => p.to_string(),
            p => format!("{p}/"),
        };
        let state = self.state.borrow();
        let mut entries = BTreeMap::<String, HostDirEntry>::new();
        for (file, contents) in &state.files {
            let Some(rest) = file.strip_prefix(&prefix) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }
            if let Some((dir, _)) = rest.split_once('/') {
                entries.entry(dir.to_string()).or_insert(HostDirEntry {
                    name: dir.to_string(),
                    kind: "directory".to_string(),
                    size_bytes: None,
                });
            } else {
                entries.insert(
                    rest.to_string(),
                    HostDirEntry {
                        name: rest.to_string(),
                        kind: "file".to_string(),
                        size_bytes: i64::try_from(contents.len()).ok(),
                    },
                );
            }
        }
        if entries.is_empty() && !state.files.contains_key(path) {
            return Err(format!("cannot list `{path}`"));
        }
        Ok(entries.into_values().collect())
    }

    fn now_millis(&self) -> u64 {
        let mut state = self.state.borrow_mut();
        let now = state.now;
        let tick = state.tick_per_poll;
        state.now += tick;
        now
    }

    fn input(&self) -> String {
        self.state.borrow().input.clone()
    }

    fn defer_error(&self, rendered: &str) {
        self.state
            .borrow_mut()
            .defer_errors
            .push(rendered.to_string());
    }

    fn extern_call(&self, module: &str, function: &str, args: &[Value]) -> Result<Value, String> {
        self.state
            .borrow()
            .extern_replay
            .call_replay_sandbox(module, function, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_captures_transcript() {
        let host = TestHost::new();
        host.print("a");
        host.print("b");
        assert_eq!(host.stdout(), vec!["a", "b"]);
    }

    #[test]
    fn test_host_virtual_files_roundtrip() {
        let host = TestHost::new();
        host.add_file("config.txt", "v=1");
        let h = host.open("config.txt").expect("open");
        assert_eq!(host.read(h).as_deref(), Ok("v=1"));
        host.write(h, "v=2").expect("write");
        assert_eq!(host.read(h).as_deref(), Ok("v=2"));
        host.close(h);
        assert!(host.read(h).is_err());
        assert_eq!(
            host.files().get("config.txt").map(String::as_str),
            Some("v=2")
        );
        host.write_bytes("raw.bin", &[0, 0x80, 0xff])
            .expect("write bytes");
        assert_eq!(host.read_bytes("raw.bin"), Ok(vec![0, 0x80, 0xff]));
    }

    #[test]
    fn test_host_open_missing_is_err() {
        let host = TestHost::new();
        assert!(host.open("nope.txt").is_err());
    }

    #[test]
    fn test_host_virtual_clock() {
        let host = TestHost::new();
        assert_eq!(host.now_millis(), 0);
        host.advance_millis(10);
        assert_eq!(host.now_millis(), 10);
        host.set_tick_per_poll(5);
        assert_eq!(host.now_millis(), 10);
        assert_eq!(host.now_millis(), 15);
    }
}
