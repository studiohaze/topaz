//! `topaz_difftest_native` — the 3-COLUMN differential harness (CDR-006 §7,
//! v5.4 native emit). It proves the NATIVE (monomorphized) backend's output
//! COMPILES, RUNS, and is BYTE-IDENTICAL to BOTH the interpreter and the boxed
//! emitter — the `run≡build` invariant extended to the native lane.
//!
//! A build script resolves + type-checks each native-eligible fixture, emits the
//! BOXED program and the NATIVE-checked program (each in its own `mod`), and the
//! crate `include!`s them — so all three columns compile against the SAME
//! `topaz_value`/`topaz_rt` types the interpreter and comparator use (one type
//! universe; raw typed `Value`/`RunOutcome` comparison is meaningful). The test
//! runs each fixture's source on the interpreter and the two emitted entries in
//! process and asserts all three agree on outcome, stdout, files, and defers.
//!
//! UNLIKE `topaz_difftest` (the checker-free boxed lane), this harness's build
//! script DOES type-check (the native backend consumes the typed HIR), and a
//! separate REFUSAL set is pinned at build time: the native backend must DECLINE
//! every shape outside the scalar island with a structured `TPZ6002`, never a
//! leaked `rustc` error from a divergent emit.

use std::rc::Rc;

use topaz_rt::{Host, RunOutcome};

#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use topaz_interp::{Machine, TestHost};
#[cfg(test)]
use topaz_resolve::{FileProvider, InMemoryProvider, resolve};
#[cfg(test)]
use topaz_value::{ExternReplayStore, values_equal};

/// One generated 3-column fixture: its name, its Topaz source (the interpreter
/// column), and the two emitted hostable entries (boxed + native).
pub struct Fixture {
    pub name: &'static str,
    pub source: &'static str,
    pub boxed: fn(Rc<dyn Host>) -> RunOutcome,
    pub native: fn(Rc<dyn Host>) -> RunOutcome,
}

/// One file of a multi-module fallback fixture.
pub struct FixtureFile {
    pub path: &'static str,
    pub source: &'static str,
}

/// One extern virtual module used by an extern fallback fixture.
pub struct ExternFixtureFile {
    pub identity: &'static str,
    pub path: &'static str,
    pub source: &'static str,
    pub replay_error: Option<&'static str>,
}

/// One generated FALLBACK fixture: native DECLINES it (asserted at build time),
/// so the CLI's `--backend native` falls back to the BOXED emit — `boxed` is that
/// exact fallback entry. The test runs interp vs boxed and asserts byte-identical,
/// proving the decline→boxed PATH (not just the decline) is correct end to end.
pub struct FallbackFixture {
    pub name: &'static str,
    pub entry: &'static str,
    pub files: &'static [FixtureFile],
    pub externs: &'static [ExternFixtureFile],
    pub extern_replay_jsonl: &'static str,
    pub boxed: fn(Rc<dyn Host>) -> RunOutcome,
}

/// One generated function-level hybrid fixture. Whole-unit native declined at
/// build time; the `hybrid` column is the boxed module envelope with only
/// admitted scalar function closures replaced.
pub struct HybridFixture {
    pub name: &'static str,
    pub entry: &'static str,
    pub files: &'static [FixtureFile],
    pub boxed: fn(Rc<dyn Host>) -> RunOutcome,
    pub hybrid: fn(Rc<dyn Host>) -> RunOutcome,
}

// The build script writes `mod fixture_N { … }` for each emitted program plus
// the `FIXTURES` table and the `FIXTURE_COUNT`/`REFUSAL_COUNT` pins.
include!(concat!(env!("OUT_DIR"), "/fixtures.rs"));

#[cfg(test)]
struct ExternReplayProvider {
    inner: InMemoryProvider,
    extern_files: BTreeMap<String, String>,
    extern_namespaces: BTreeSet<String>,
    replay_errors: BTreeMap<String, String>,
}

#[cfg(test)]
impl ExternReplayProvider {
    fn new() -> Self {
        Self {
            inner: InMemoryProvider::new(),
            extern_files: BTreeMap::new(),
            extern_namespaces: BTreeSet::new(),
            replay_errors: BTreeMap::new(),
        }
    }

    fn add_file(&mut self, path: &'static str, source: &'static str) {
        self.inner.add_file(path, source);
    }

    fn add_extern_file(&mut self, file: &ExternFixtureFile) {
        self.inner.add_file(file.path, file.source);
        self.extern_files
            .insert(file.path.to_string(), file.identity.to_string());
        if let Some((root, _)) = file.identity.split_once('.') {
            self.extern_namespaces.insert(root.to_string());
        }
        if let Some(error) = file.replay_error {
            self.replay_errors
                .insert(file.identity.to_string(), error.to_string());
        }
    }
}

#[cfg(test)]
impl FileProvider for ExternReplayProvider {
    fn read(&self, path: &str) -> topaz_resolve::SourceRead {
        self.inner.read(path)
    }

    fn is_extern_file(&self, path: &str) -> bool {
        self.extern_files.contains_key(path)
    }

    fn is_extern_namespace(&self, identity: &str) -> bool {
        self.extern_namespaces
            .iter()
            .any(|ns| identity == ns || identity.starts_with(&format!("{ns}.")))
    }

    fn extern_replay_error(&self, identity: &str) -> Option<String> {
        self.replay_errors.get(identity).cloned()
    }

    fn read_directory(&self, dir: &str) -> topaz_resolve::DirectoryRead {
        self.inner.read_directory(dir)
    }

    fn physical_id(&self, path: &str) -> Option<String> {
        self.inner.physical_id(path)
    }
}

/// Run a fixture's source on the interpreter, returning the structured outcome
/// plus the host transcript and final virtual files — the interpreter column.
#[cfg(test)]
fn run_interpreter(
    source: &str,
) -> (
    RunOutcome,
    Vec<String>,
    BTreeMap<String, String>,
    Vec<String>,
) {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", source);
    let unit = resolve(&provider, "main.tpz", None);
    let host = TestHost::new();
    host.add_file("input.txt", "hello");
    let outcome = match Machine::run_unit(&unit, &host) {
        Ok(value) => RunOutcome::Completed(value),
        Err(error) => RunOutcome::Faulted(error),
    };
    (outcome, host.stdout(), host.files(), host.defer_errors())
}

#[cfg(test)]
fn run_interpreter_unit(
    entry: &str,
    files: &[FixtureFile],
    externs: &[ExternFixtureFile],
    extern_replay_jsonl: &str,
) -> (
    RunOutcome,
    Vec<String>,
    BTreeMap<String, String>,
    Vec<String>,
) {
    let mut provider = ExternReplayProvider::new();
    for file in files {
        provider.add_file(file.path, file.source);
    }
    for file in externs {
        provider.add_extern_file(file);
    }
    let unit = resolve(&provider, entry, None);
    let host = TestHost::new();
    host.add_file("input.txt", "hello");
    if !extern_replay_jsonl.trim().is_empty() {
        let replay = ExternReplayStore::parse_jsonl(extern_replay_jsonl)
            .expect("extern replay JSONL fixture parses");
        host.set_extern_replay(replay);
    }
    let outcome = match Machine::run_unit(&unit, &host) {
        Ok(value) => RunOutcome::Completed(value),
        Err(error) => RunOutcome::Faulted(error),
    };
    (outcome, host.stdout(), host.files(), host.defer_errors())
}

/// Outcome identity (CDR-006 §3): completed values through the shared
/// comparator; faults by code + message + span byte offsets — the SAME identity
/// `topaz_difftest` uses, so the two harnesses agree on what "byte-identical"
/// means.
#[cfg(test)]
fn outcomes_match(a: &RunOutcome, b: &RunOutcome) -> bool {
    match (a, b) {
        (RunOutcome::Completed(x), RunOutcome::Completed(y)) => values_equal(x, y) == Ok(true),
        (RunOutcome::Faulted(x), RunOutcome::Faulted(y)) => {
            x.code == y.code
                && x.message == y.message
                && x.span.file == y.span.file
                && x.span.lo == y.span.lo
                && x.span.hi == y.span.hi
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run one emitted entry on a fresh, identically-seeded host, returning the
    /// outcome and the host's transcripts.
    fn run_emitted(
        run: fn(Rc<dyn Host>) -> RunOutcome,
    ) -> (
        RunOutcome,
        Vec<String>,
        BTreeMap<String, String>,
        Vec<String>,
    ) {
        let host = Rc::new(TestHost::new());
        host.add_file("input.txt", "hello");
        let outcome = run(host.clone());
        (outcome, host.stdout(), host.files(), host.defer_errors())
    }

    fn run_emitted_with_replay(
        run: fn(Rc<dyn Host>) -> RunOutcome,
        extern_replay_jsonl: &str,
    ) -> (
        RunOutcome,
        Vec<String>,
        BTreeMap<String, String>,
        Vec<String>,
    ) {
        let host = Rc::new(TestHost::new());
        host.add_file("input.txt", "hello");
        if !extern_replay_jsonl.trim().is_empty() {
            let replay = ExternReplayStore::parse_jsonl(extern_replay_jsonl)
                .expect("extern replay JSONL fixture parses");
            host.set_extern_replay(replay);
        }
        let outcome = run(host.clone());
        (outcome, host.stdout(), host.files(), host.defer_errors())
    }

    #[test]
    fn native_matches_interpreter_and_boxed() {
        // The non-`Send` graph lives inside one spawned thread; only the Send
        // failure list crosses the join. A generous stack so emitted code can
        // reach `CALL_DEPTH_LIMIT` cleanly (parity with `topaz_difftest`).
        let failures: Vec<String> = std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
                let mut failures = Vec::new();
                for fixture in FIXTURES {
                    let (interp, i_out, i_files, i_defer) = run_interpreter(fixture.source);
                    let (boxed, b_out, b_files, b_defer) = run_emitted(fixture.boxed);
                    let (native, n_out, n_files, n_defer) = run_emitted(fixture.native);

                    // Column 1 vs 2: interpreter vs boxed (the existing invariant).
                    if !outcomes_match(&interp, &boxed) {
                        failures.push(format!(
                            "{}: interp != boxed\n    interp: {interp:?}\n    boxed:  {boxed:?}",
                            fixture.name
                        ));
                    }
                    // Column 1 vs 3: interpreter vs NATIVE (the new invariant).
                    if !outcomes_match(&interp, &native) {
                        failures.push(format!(
                            "{}: interp != native\n    interp: {interp:?}\n    native: {native:?}",
                            fixture.name
                        ));
                    }
                    // Column 2 vs 3: boxed vs native (transitive, but pinned so a
                    // failure localizes precisely).
                    if !outcomes_match(&boxed, &native) {
                        failures.push(format!(
                            "{}: boxed != native\n    boxed:  {boxed:?}\n    native: {native:?}",
                            fixture.name
                        ));
                    }
                    // All three transcripts must agree.
                    if !(i_out == b_out && i_out == n_out) {
                        failures.push(format!(
                            "{}: stdout differs\n    interp: {i_out:?}\n    boxed:  {b_out:?}\n    native: {n_out:?}",
                            fixture.name
                        ));
                    }
                    if !(i_files == b_files && i_files == n_files) {
                        failures.push(format!("{}: virtual file state differs", fixture.name));
                    }
                    if !(i_defer == b_defer && i_defer == n_defer) {
                        failures.push(format!("{}: defer-error transcript differs", fixture.name));
                    }
                }
                failures
            })
            .expect("spawn harness thread")
            .join()
            .expect("harness thread panicked");

        assert!(
            failures.is_empty(),
            "native differential mismatches ({} of {}):\n{}",
            failures.len(),
            FIXTURES.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn native_fallback_matches_interpreter() {
        // Native declines each row at build time and this test proves the boxed
        // fallback path remains byte-identical to the interpreter.
        let failures: Vec<String> = std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
                let mut failures = Vec::new();
                for fixture in FALLBACK_FIXTURES {
                    let (interp, i_out, i_files, i_defer) = run_interpreter_unit(
                        fixture.entry,
                        fixture.files,
                        fixture.externs,
                        fixture.extern_replay_jsonl,
                    );
                    let (boxed, b_out, b_files, b_defer) =
                        run_emitted_with_replay(fixture.boxed, fixture.extern_replay_jsonl);
                    if !outcomes_match(&interp, &boxed) {
                        failures.push(format!(
                            "{}: interp != boxed-fallback\n    interp: {interp:?}\n    boxed:  {boxed:?}",
                            fixture.name
                        ));
                    }
                    if !(i_out == b_out && i_files == b_files && i_defer == b_defer) {
                        failures.push(format!("{}: transcript/file/defer differs", fixture.name));
                    }
                }
                failures
            })
            .expect("spawn harness thread")
            .join()
            .expect("harness thread panicked");
        assert!(
            failures.is_empty(),
            "fallback mismatches ({} of {}):\n{}",
            failures.len(),
            FALLBACK_FIXTURES.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn bounded_hybrid_matches_interpreter_and_boxed() {
        let failures: Vec<String> = std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
                let mut failures = Vec::new();
                for fixture in HYBRID_FIXTURES {
                    let (interp, i_out, i_files, i_defer) =
                        run_interpreter_unit(fixture.entry, fixture.files, &[], "");
                    let (boxed, b_out, b_files, b_defer) = run_emitted(fixture.boxed);
                    let (hybrid, h_out, h_files, h_defer) = run_emitted(fixture.hybrid);
                    if !outcomes_match(&interp, &boxed) {
                        failures.push(format!(
                            "{}: interp != boxed\n    interp: {interp:?}\n    boxed:  {boxed:?}",
                            fixture.name
                        ));
                    }
                    if !outcomes_match(&interp, &hybrid) {
                        failures.push(format!(
                            "{}: interp != hybrid\n    interp: {interp:?}\n    hybrid: {hybrid:?}",
                            fixture.name
                        ));
                    }
                    if !outcomes_match(&boxed, &hybrid) {
                        failures.push(format!(
                            "{}: boxed != hybrid\n    boxed:  {boxed:?}\n    hybrid: {hybrid:?}",
                            fixture.name
                        ));
                    }
                    if !(i_out == b_out
                        && i_out == h_out
                        && i_files == b_files
                        && i_files == h_files
                        && i_defer == b_defer
                        && i_defer == h_defer)
                    {
                        failures.push(format!("{}: transcript/file/defer differs", fixture.name));
                    }
                }
                failures
            })
            .expect("spawn hybrid harness thread")
            .join()
            .expect("hybrid harness thread panicked");
        assert!(
            failures.is_empty(),
            "hybrid differential mismatches ({} of {}):\n{}",
            failures.len(),
            HYBRID_FIXTURES.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn the_native_eligible_set_is_pinned() {
        // An EXACT count, not a floor: a `>=` floor would let a fixture be
        // silently dropped without failing. Bumping this is the deliberate act
        // of growing the native-eligible set.
        assert_eq!(
            FIXTURE_COUNT, 224,
            "the native-eligible fixture set drifted"
        );
        assert_eq!(FIXTURES.len(), FIXTURE_COUNT);
    }

    #[test]
    fn the_native_refusal_set_is_pinned() {
        // The build script asserted each REFUSAL fixture declines with a
        // structured TPZ6002; this pins the COUNT so a refusal is never silently
        // dropped (which would let a divergent native emit through).
        assert_eq!(REFUSAL_COUNT, 0, "the native-refusal fixture set drifted");
    }

    #[test]
    fn the_native_fallback_set_is_pinned() {
        // Pin the fallback count so a native decline-to-boxed case is never
        // silently dropped.
        assert_eq!(FALLBACK_COUNT, 3, "the native-fallback fixture set drifted");
        assert_eq!(FALLBACK_FIXTURES.len(), FALLBACK_COUNT);
    }

    #[test]
    fn the_bounded_hybrid_set_is_pinned() {
        assert_eq!(HYBRID_COUNT, 9, "the hybrid fixture set drifted");
        assert_eq!(HYBRID_FIXTURES.len(), HYBRID_COUNT);
    }
}
