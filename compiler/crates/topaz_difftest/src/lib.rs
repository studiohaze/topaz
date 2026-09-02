//! `topaz_difftest` — differential harness #1 (CDR-006 §7). It proves
//! the emitter's output COMPILES and RUNS and matches the interpreter.
//!
//! A build script emits each eligible fixture as `emit_module` output
//! into one generated file, each wrapped in its own `mod fixture_N`,
//! which this crate `include!`s — so the emitted programs compile as
//! part of the workspace against the SAME `topaz_value`/`topaz_rt`
//! types the interpreter and the comparator use (one type universe;
//! raw typed `Value`/`RunOutcome` comparison is meaningful). A green
//! build is therefore the compile-shape proof; the test then runs each
//! program through both engines and compares the outcome in-process.

use std::rc::Rc;

use topaz_rt::{Host, RunOutcome, Value};

#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use topaz_interp::{Machine, TestHost};
#[cfg(test)]
use topaz_resolve::{FileProvider, InMemoryProvider, resolve, resolve_with_version};
use topaz_syntax::LangVersion;
#[cfg(test)]
use topaz_value::{ExternReplayStore, values_equal};

/// One generated fixture: its name, its Topaz source (run on the
/// interpreter side), and the emitted program's hostable entry.
pub struct Fixture {
    pub name: &'static str,
    pub source: &'static str,
    /// Acceptable stdout transcripts for an implementation-defined fixture
    /// (§15 `concurrent` cross-arm interleaving). Empty = exact interp==emit
    /// comparison (the default for every deterministic fixture).
    pub stdout_alts: &'static [&'static [&'static str]],
    pub run: fn(Rc<dyn Host>) -> RunOutcome,
    pub call_export: fn(Rc<dyn Host>, &str, Vec<Value>) -> RunOutcome,
}

/// One file of a multi-module fixture (its in-unit path + source).
pub struct FixtureFile {
    pub path: &'static str,
    pub source: &'static str,
}

/// One extern virtual module used by an extern replay fixture.
pub struct ExternFixtureFile {
    pub identity: &'static str,
    pub path: &'static str,
    pub source: &'static str,
    pub replay_error: Option<&'static str>,
}

/// One generated MULTI-MODULE fixture (CDR-006 §7 E-3): its files (run on
/// the interpreter side as a resolved unit) + the emitted program's entry.
/// A multi-module unit still lowers to ONE hostable `run_with_host` (the
/// emitter inlines each non-entry module's export record into the entry
/// body), so it fits the same one-`mod`-per-fixture shape as a single file.
pub struct ModuleFixture {
    pub name: &'static str,
    pub entry: &'static str,
    pub language_version: LangVersion,
    pub files: &'static [FixtureFile],
    pub externs: &'static [ExternFixtureFile],
    pub extern_replay_jsonl: &'static str,
    pub run: fn(Rc<dyn Host>) -> RunOutcome,
}

// The build script writes `mod fixture_N { … }` for each fixture plus
// the `FIXTURES` and `MODULE_FIXTURES` tables referencing them.
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

/// Run a fixture's source on the interpreter, returning the structured
/// outcome plus the host transcript and final virtual files. The
/// interpreter returns `Result<Value, RtError>`; wrap it into the
/// shared `RunOutcome` the emitted `run_with_host` already returns.
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
    // §22.3 seed a known file so a fixture can `open`/`read`/`write` it (the
    // TestHost only opens files that already exist). Both engines seed the
    // SAME file, so the final `files()` comparison stays meaningful.
    host.add_file("input.txt", "hello");
    let outcome = match Machine::run_unit(&unit, &host) {
        Ok(value) => RunOutcome::Completed(value),
        Err(error) => RunOutcome::Faulted(error),
    };
    // §14 the deferred-error side channel is part of run≡build: a deferred action
    // that faults / returns `Value::Err` routes here (not the result), so both
    // engines must produce the SAME `defer_errors` transcript.
    (outcome, host.stdout(), host.files(), host.defer_errors())
}

#[cfg(test)]
fn run_interpreter_export(source: &str, name: &str, args: Vec<Value>) -> RunOutcome {
    let mut provider = InMemoryProvider::new();
    provider.add_file("main.tpz", source);
    let unit = resolve(&provider, "main.tpz", None);
    match Machine::run_unit_export(&unit, &TestHost::new(), name, args) {
        Ok(value) => RunOutcome::Completed(value),
        Err(error) => RunOutcome::Faulted(error),
    }
}

/// Run a MULTI-MODULE fixture's files on the interpreter as a resolved unit
/// (CDR-006 §7 E-3): seed every file, resolve `entry`, then run the unit —
/// `Machine::run_unit` initializes modules in unit order and returns the
/// entry result, the same target the emitted `run_with_host` drives.
#[cfg(test)]
fn run_interpreter_unit(
    entry: &str,
    language_version: LangVersion,
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
    let unit = resolve_with_version(&provider, entry, None, language_version);
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
/// comparator; faults by code + message + span byte offsets. The
/// fixtures are single-file, so the file is implicit — E-3 adds the
/// root-relative file name for multi-module units.
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

/// True if `stdout` exactly equals one of the acceptable transcripts. For §15
/// `concurrent` fixtures whose cross-arm interleaving is implementation-defined
/// (SPEC §15): each engine's transcript must be ONE of a small, fully enumerated
/// set — no wildcards/permutation helpers, since an over-wide set would mask a
/// real divergence.
#[cfg(test)]
fn stdout_in_alts(stdout: &[String], alts: &[&[&str]]) -> bool {
    alts.iter().any(|alt| {
        alt.len() == stdout.len() && alt.iter().zip(stdout.iter()).all(|(e, a)| *e == a.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use topaz_interp::machine::codes;

    #[test]
    fn malformed_external_nominal_spread_faults_in_both_engines() {
        let fixture = FIXTURES
            .iter()
            .find(|fixture| fixture.name == "record_spread_external_malformed_fields")
            .expect("external nominal spread fixture");
        let assert_fault = |input: Value, code: &'static str, message: &str| {
            let interpreted = run_interpreter_export(fixture.source, "copy", vec![input.clone()]);
            let emitted = (fixture.call_export)(Rc::new(TestHost::new()), "copy", vec![input]);

            assert!(
                outcomes_match(&interpreted, &emitted),
                "external nominal spread differs\n    interp: {interpreted:?}\n    emit:   {emitted:?}"
            );
            let RunOutcome::Faulted(error) = interpreted else {
                panic!("malformed external nominal spread must fault: {interpreted:?}");
            };
            assert_eq!(error.code, code);
            assert_eq!(error.message, message);
        };
        assert_fault(
            Value::nominal_record_with_identities(
                "User",
                "__entry__::User",
                None::<&str>,
                [(Rc::from("name"), Value::str("Ada"))],
            ),
            codes::GUARD_ARITY,
            "record `User` is missing field `age`",
        );
        assert_fault(
            Value::nominal_record_with_identities(
                "User",
                "__entry__::User",
                None::<&str>,
                [
                    (Rc::from("name"), Value::str("Ada")),
                    (Rc::from("name"), Value::str("Grace")),
                    (Rc::from("age"), Value::Int(36)),
                ],
            ),
            codes::GUARD_ARITY,
            "field `name` is given twice in `User`",
        );
        assert_fault(
            Value::nominal_record_with_identities(
                "User",
                "__entry__::User",
                None::<&str>,
                [
                    (Rc::from("name"), Value::str("Ada")),
                    (Rc::from("age"), Value::Int(36)),
                    (Rc::from("rank"), Value::Int(1)),
                ],
            ),
            codes::GUARD_NO_FIELD,
            "record `User` has no field `rank`",
        );
    }

    #[test]
    fn emitted_programs_match_the_interpreter() {
        // CDR-006 §4: the non-`Send` graph (values, hosts, futures) and
        // the comparison live INSIDE one spawned thread; only a `Send`
        // verdict (the failure list) crosses the join. Each engine gets
        // its OWN `TestHost`, seeded identically, so transcripts cannot
        // cross-contaminate.
        // A generous stack (≥ the real `topaz build` binary's main-thread stack) so the
        // emitted code can reach `CALL_DEPTH_LIMIT` and fault `GUARD_RECURSION` cleanly
        // in-process — cargo's default 2 MiB test thread would overflow first, BELOW the
        // real binary's reach, making the harness less permissive than reality (§4).
        let failures: Vec<String> = std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
            let mut failures = Vec::new();
            for fixture in FIXTURES {
                let (interp_outcome, interp_stdout, interp_files, interp_defer_errors) =
                    run_interpreter(fixture.source);

                let emit_host = Rc::new(TestHost::new());
                emit_host.add_file("input.txt", "hello");
                let emit_outcome = (fixture.run)(emit_host.clone());

                if !outcomes_match(&interp_outcome, &emit_outcome) {
                    failures.push(format!(
                        "{}: outcome differs\n    interp: {interp_outcome:?}\n    emit:   {emit_outcome:?}",
                        fixture.name
                    ));
                }
                let emit_stdout = emit_host.stdout();
                if fixture.stdout_alts.is_empty() {
                    if emit_stdout != interp_stdout {
                        failures.push(format!("{}: stdout transcript differs", fixture.name));
                    }
                } else if !stdout_in_alts(&interp_stdout, fixture.stdout_alts)
                    || !stdout_in_alts(&emit_stdout, fixture.stdout_alts)
                {
                    failures.push(format!(
                        "{}: stdout transcript not in the accepted set\n    interp: {interp_stdout:?}\n    emit:   {emit_stdout:?}",
                        fixture.name
                    ));
                }
                if emit_host.files() != interp_files {
                    failures.push(format!("{}: virtual file state differs", fixture.name));
                }
                if emit_host.defer_errors() != interp_defer_errors {
                    failures.push(format!(
                        "{}: defer-error transcript differs\n    interp: {interp_defer_errors:?}\n    emit:   {:?}",
                        fixture.name,
                        emit_host.defer_errors()
                    ));
                }
            }
            failures
        })
        .expect("spawn harness thread")
        .join()
        .expect("harness thread panicked");

        assert!(
            failures.is_empty(),
            "differential mismatches ({} of {}):\n{}",
            failures.len(),
            FIXTURES.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn af006_using_resource_unwind_and_order_receipts_are_pinned() {
        for name in [
            "using_resource_closes_before_later_closure",
            "using_resource_question_closes_before_later_closure",
            "using_resource_return_closes_before_later_closure",
            "using_resource_break_closes_before_later_closure",
            "using_resource_continue_closes_before_later_closure",
        ] {
            let fixture = FIXTURES
                .iter()
                .find(|fixture| fixture.name == name)
                .unwrap_or_else(|| panic!("missing deferred-action fixture {name}"));
            let (outcome, _, _, _) = run_interpreter(fixture.source);
            assert!(
                matches!(outcome, RunOutcome::Completed(Value::Str(ref text)) if &**text == "file is closed"),
                "{name} must observe the implicit close after the control transfer: {outcome:?}"
            );
        }

        let order = FIXTURES
            .iter()
            .find(|fixture| fixture.name == "using_resource_body_defer_runs_before_close")
            .expect("missing defer-order fixture");
        let (outcome, stdout, _, _) = run_interpreter(order.source);
        assert!(
            matches!(outcome, RunOutcome::Completed(Value::Str(ref text)) if &**text == "file is closed"),
            "using must close after its body defers: {outcome:?}"
        );
        assert_eq!(stdout, ["body", "defer:hello"]);

        let once = FIXTURES
            .iter()
            .find(|fixture| {
                fixture.name == "using_resource_initializer_once_and_body_tail_discarded"
            })
            .expect("missing single-acquisition fixture");
        let (outcome, _, _, _) = run_interpreter(once.source);
        assert!(
            matches!(outcome, RunOutcome::Completed(Value::Ok(ref value)) if matches!(&**value, Value::Int(1))),
            "initializer must run once and the body tail must be discarded: {outcome:?}"
        );
    }

    #[test]
    fn af002_nominal_record_dynamic_defaults_are_pinned() {
        for name in [
            "nominal_record_mutable_default_current_binding",
            "nominal_record_effectful_defaults_order_and_skip",
        ] {
            let fixture = FIXTURES
                .iter()
                .find(|fixture| fixture.name == name)
                .unwrap_or_else(|| panic!("missing wide fixture {name}"));
            let (interp_outcome, interp_stdout, interp_files, interp_defer_errors) =
                run_interpreter(fixture.source);
            let emit_host = Rc::new(TestHost::new());
            emit_host.add_file("input.txt", "hello");
            let emit_outcome = (fixture.run)(emit_host.clone());
            assert!(
                outcomes_match(&interp_outcome, &emit_outcome),
                "{name} outcome differs: interp={interp_outcome:?}, emit={emit_outcome:?}"
            );
            assert_eq!(emit_host.stdout(), interp_stdout, "{name} stdout");
            assert_eq!(emit_host.files(), interp_files, "{name} files");
            assert_eq!(
                emit_host.defer_errors(),
                interp_defer_errors,
                "{name} defer errors"
            );
            if name == "nominal_record_effectful_defaults_order_and_skip" {
                assert_eq!(interp_stdout, ["2", "1", "2"]);
            }
        }

        let fixture = MODULE_FIXTURES
            .iter()
            .find(|fixture| fixture.name == "mod_nominal_record_mutable_default_defining_scope")
            .expect("missing module fixture");
        let (interp_outcome, interp_stdout, interp_files, interp_defer_errors) =
            run_interpreter_unit(
                fixture.entry,
                fixture.language_version,
                fixture.files,
                fixture.externs,
                fixture.extern_replay_jsonl,
            );
        let emit_host = Rc::new(TestHost::new());
        emit_host.add_file("input.txt", "hello");
        let emit_outcome = (fixture.run)(emit_host.clone());
        assert!(
            outcomes_match(&interp_outcome, &emit_outcome),
            "module outcome differs: interp={interp_outcome:?}, emit={emit_outcome:?}"
        );
        assert_eq!(emit_host.stdout(), interp_stdout);
        assert_eq!(emit_host.files(), interp_files);
        assert_eq!(emit_host.defer_errors(), interp_defer_errors);
    }

    #[test]
    fn wrong_nominal_record_spread_skips_replacement_effect() {
        let fixture = FIXTURES
            .iter()
            .find(|fixture| fixture.name == "record_spread_wrong_id_fault")
            .expect("wrong-identity spread fixture is pinned");

        let (interp_outcome, interp_stdout, _, _) = run_interpreter(fixture.source);
        assert!(
            matches!(interp_outcome, RunOutcome::Faulted(_)),
            "wrong nominal spread must fault before replacement"
        );
        assert!(
            interp_stdout.is_empty(),
            "interpreter evaluated the replacement after the wrong-id guard: {interp_stdout:?}"
        );

        let emit_host = Rc::new(TestHost::new());
        let emit_outcome = (fixture.run)(emit_host.clone());
        assert!(
            matches!(emit_outcome, RunOutcome::Faulted(_)),
            "boxed Rust wrong nominal spread must fault before replacement"
        );
        assert!(
            emit_host.stdout().is_empty(),
            "boxed Rust evaluated the replacement after the wrong-id guard: {:?}",
            emit_host.stdout()
        );
    }

    #[test]
    fn nominal_record_spread_shallow_copy_reuses_field_values() {
        let fixture = FIXTURES
            .iter()
            .find(|fixture| fixture.name == "record_spread_shallow_array_alias")
            .expect("shallow-alias fixture is pinned");
        let expected = vec!["2/2".to_string(), "false".to_string()];

        let (interp_outcome, interp_stdout, _, _) = run_interpreter(fixture.source);
        assert!(matches!(interp_outcome, RunOutcome::Completed(_)));
        assert_eq!(interp_stdout, expected, "interpreter shallow-copy receipt");

        let emit_host = Rc::new(TestHost::new());
        let emit_outcome = (fixture.run)(emit_host.clone());
        assert!(matches!(emit_outcome, RunOutcome::Completed(_)));
        assert_eq!(
            emit_host.stdout(),
            expected,
            "boxed Rust shallow-copy receipt"
        );
    }

    #[test]
    fn emitted_module_programs_match_the_interpreter() {
        // The E-3 multi-module proof: the SAME `emit_module` output that the
        // build ALREADY compiles (so the multi-module emit shape is proven) is
        // run here and compared to the interpreter over the resolved unit, the
        // same as the single-file harness. This turns multi-module native emit
        // from "compiles, unverified" into "run≡build pinned".
        // A generous stack (≥ the real `topaz build` binary's main-thread stack) so the
        // emitted code can reach `CALL_DEPTH_LIMIT` and fault `GUARD_RECURSION` cleanly
        // in-process — cargo's default 2 MiB test thread would overflow first, BELOW the
        // real binary's reach, making the harness less permissive than reality (§4).
        let failures: Vec<String> = std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
            let mut failures = Vec::new();
            for fixture in MODULE_FIXTURES {
                let (interp_outcome, interp_stdout, interp_files, interp_defer_errors) =
                    run_interpreter_unit(
                        fixture.entry,
                        fixture.language_version,
                        fixture.files,
                        fixture.externs,
                        fixture.extern_replay_jsonl,
                    );
                if fixture.name == "v520_module_stable_nominals_and_imported_typed_json" {
                    assert!(
                        matches!(&interp_outcome, RunOutcome::Completed(Value::Int(42))),
                        "{} must produce the specified 5.20 result, got {interp_outcome:?}",
                        fixture.name
                    );
                }

                let emit_host = Rc::new(TestHost::new());
                emit_host.add_file("input.txt", "hello");
                if !fixture.extern_replay_jsonl.trim().is_empty() {
                    let replay = ExternReplayStore::parse_jsonl(fixture.extern_replay_jsonl)
                        .expect("extern replay JSONL fixture parses");
                    emit_host.set_extern_replay(replay);
                }
                let emit_outcome = (fixture.run)(emit_host.clone());

                if !outcomes_match(&interp_outcome, &emit_outcome) {
                    failures.push(format!(
                        "{}: outcome differs\n    interp: {interp_outcome:?}\n    emit:   {emit_outcome:?}",
                        fixture.name
                    ));
                }
                if emit_host.stdout() != interp_stdout {
                    failures.push(format!("{}: stdout transcript differs", fixture.name));
                }
                if emit_host.files() != interp_files {
                    failures.push(format!("{}: virtual file state differs", fixture.name));
                }
                if emit_host.defer_errors() != interp_defer_errors {
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
            "module differential mismatches ({} of {}):\n{}",
            failures.len(),
            MODULE_FIXTURES.len(),
            failures.join("\n")
        );
    }
}
