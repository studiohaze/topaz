//! `topaz_difftest_py` — Python backend differential harness for the v5.5 line.
//!
//! This crate is the release-facing line-breaker witness Python column. Each
//! fixture is resolved as ordinary Topaz source, emitted through `topaz_emit_py`,
//! run under pinned CPython 3.13.14 through `topaz_py_rt`, and compared against
//! both the interpreter and the checked-in Rust reference for the same corpus.

#![cfg_attr(not(test), allow(dead_code))]
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use topaz_emit_py::{
    CheckedAliasSurfaces, PY_RT, emit_module,
    emit_module_with_checked_aliases_and_extern_replay_and_policies,
};
use topaz_resolve::{PhysicalProvider, resolve_with_version};
use topaz_rt::{Host, RunOutcome};
use topaz_syntax::LangVersion;
use topaz_syntax::ast::StmtKind;

mod runner;
mod trace;

const PYTHON_FIXTURE_COUNT: usize = 5;
const BADNESS_CASE_COUNT: usize = 5760;
const JUST_LATIN_CASE_COUNT: usize = 14;
const JUST_CASE_COUNT: usize = 32;
const LINEBREAK_CLASSIFY_CASE_COUNT: usize = 95;
const DP_CASE_COUNT: usize = 38;
const PYTHON_CASE_COUNT: usize = BADNESS_CASE_COUNT
    + JUST_LATIN_CASE_COUNT
    + JUST_CASE_COUNT
    + LINEBREAK_CLASSIFY_CASE_COUNT
    + DP_CASE_COUNT;
const PYTHON_WIDE_CORE_FIXTURE_COUNT: usize = 926;
const PYTHON_MODULE_CORE_FIXTURE_COUNT: usize = 151;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CorpusKind {
    Badness,
    JustLatin,
    Just,
    LinebreakClassify,
    Dp,
}

#[derive(Debug, Clone, Copy)]
struct FixtureSpec {
    name: &'static str,
    entry: &'static str,
    reference: &'static str,
    runner: &'static str,
    kind: CorpusKind,
    expected_cases: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WideCoreKind {
    Regular,
    FileConfig,
}

#[derive(Clone, Copy)]
struct WideCoreFixture {
    name: &'static str,
    source: &'static str,
    kind: WideCoreKind,
    run: fn(Rc<dyn Host>) -> RunOutcome,
}

struct ModuleCoreFixture {
    name: &'static str,
    entry: &'static str,
    files: &'static [(&'static str, &'static str)],
    run: fn(Rc<dyn Host>) -> RunOutcome,
}

include!(concat!(env!("OUT_DIR"), "/wide_core.rs"));
#[derive(Debug, Clone)]
struct Case {
    name: String,
    input: String,
}

const FIXTURES: &[FixtureSpec] = &[
    FixtureSpec {
        name: "badness",
        entry: "fixtures/topaz_emit_py/atlas-poc/badness.tpz",
        reference: "fixtures/topaz_emit_py/atlas-poc/badness_fp.rs",
        runner: "fixtures/topaz_emit_py/atlas-poc/badness-runner.py",
        kind: CorpusKind::Badness,
        expected_cases: BADNESS_CASE_COUNT,
    },
    FixtureSpec {
        name: "just_latin",
        entry: "fixtures/topaz_emit_py/atlas-poc/just_latin.tpz",
        reference: "fixtures/topaz_emit_py/atlas-poc/just_latin_fp.rs",
        runner: "fixtures/topaz_emit_py/atlas-poc/just-latin-runner.py",
        kind: CorpusKind::JustLatin,
        expected_cases: JUST_LATIN_CASE_COUNT,
    },
    FixtureSpec {
        name: "just",
        entry: "fixtures/topaz_emit_py/atlas-poc/just.tpz",
        reference: "fixtures/topaz_emit_py/atlas-poc/just_fp.rs",
        runner: "fixtures/topaz_emit_py/atlas-poc/just-runner.py",
        kind: CorpusKind::Just,
        expected_cases: JUST_CASE_COUNT,
    },
    FixtureSpec {
        name: "linebreak_classify",
        entry: "fixtures/topaz_emit_py/atlas-poc/linebreak-classify.tpz",
        reference: "fixtures/topaz_emit_py/atlas-poc/oracle.rs",
        runner: "fixtures/topaz_emit_py/atlas-poc/parity-runner.py",
        kind: CorpusKind::LinebreakClassify,
        expected_cases: LINEBREAK_CLASSIFY_CASE_COUNT,
    },
    FixtureSpec {
        name: "dp",
        entry: "fixtures/topaz_emit_py/atlas-poc/dp.tpz",
        reference: "fixtures/topaz_emit_py/atlas-poc/dp_fp.rs",
        runner: "fixtures/topaz_emit_py/atlas-poc/dp-runner.py",
        kind: CorpusKind::Dp,
        expected_cases: DP_CASE_COUNT,
    },
];

#[cfg(test)]
mod tests;
