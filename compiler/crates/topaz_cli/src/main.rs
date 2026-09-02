//! `topaz` — command-line interface for the Topaz compiler and toolchain.
//!
//! It parses, formats, checks, runs, emits, builds, serves, and inspects Topaz
//! packages and compiler products. Repository-only commands validate the
//! maintained corpora, fixtures, and embedded compiler inputs.

mod artifact;
mod compiler_support;
mod lispex;
mod lispex_embed;
mod profile;
mod storage;

#[path = "build/environment.rs"]
mod build_environment;
#[path = "build/native.rs"]
mod build_native;
#[path = "build/python.rs"]
mod build_python;
#[path = "build/service.rs"]
mod build_service;
#[path = "command/dispatch.rs"]
mod command_dispatch;
#[path = "command/flags.rs"]
mod command_flags;
#[path = "command/validation.rs"]
mod command_validation;
#[path = "commands/bench.rs"]
mod commands_bench;
#[path = "commands/build.rs"]
mod commands_build;
#[path = "commands/dev.rs"]
mod commands_dev;
#[path = "commands/doc.rs"]
mod commands_doc;
#[path = "commands/emit.rs"]
mod commands_emit;
#[path = "commands/fmt.rs"]
mod commands_fmt;
#[path = "commands/refactor.rs"]
mod commands_refactor;
#[path = "commands/run.rs"]
mod commands_run;
#[path = "commands/test.rs"]
mod commands_test;
#[path = "compile/check.rs"]
mod compile_check;
#[path = "compile/lower.rs"]
mod compile_lower;
#[path = "compile/observation.rs"]
mod compile_observation;
#[path = "compile/self_host.rs"]
mod compile_self_host;
#[path = "corpus.rs"]
mod corpus;
#[path = "lsp/actions.rs"]
mod lsp_actions;
#[path = "lsp/completion.rs"]
mod lsp_completion;
#[path = "lsp/protocol.rs"]
mod lsp_protocol;
#[path = "lsp/self_host.rs"]
mod lsp_self_host;
#[path = "lsp/signature.rs"]
mod lsp_signature;
#[path = "lsp/symbols.rs"]
mod lsp_symbols;
#[path = "package/dependency.rs"]
mod package_dependency;
#[path = "package/init.rs"]
mod package_init;
#[path = "package/lock.rs"]
mod package_lock;
#[path = "package/target.rs"]
mod package_target;
#[path = "package/vendor.rs"]
mod package_vendor;
#[path = "web/assets.rs"]
mod web_assets;
#[path = "web/package.rs"]
mod web_package;
#[path = "web/types.rs"]
mod web_types;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use topaz_diag::{
    Code, Diagnostic, FileId, Label, MAX_SOURCE_LEN, SourceMap, SourceMapError, Span, explain_code,
    has_errors, is_explain_code_shape, render, render_explain, render_explain_json, render_json,
};
use topaz_host_native::NativeHost;
use topaz_interp::{Host, Machine, Value};
use topaz_parser::{ParseOptions, parse, parse_with_options};
use topaz_resolve::{FileProvider, InMemoryProvider, PhysicalProvider, resolve_with_version};
use topaz_syntax::{LangVersion, ast};

use compiler_support::{
    CompilerIntent, CompilerSelection, PreflightRequest, ResolvedCompilerSelection, SelectionOrigin,
};

use build_environment::*;
use build_native::*;
use build_python::*;
use build_service::*;
use command_dispatch::*;
use command_flags::*;
use command_validation::*;
use commands_bench::*;
use commands_build::*;
use commands_dev::*;
use commands_doc::*;
use commands_emit::*;
use commands_fmt::*;
use commands_refactor::*;
use commands_run::*;
use commands_test::*;
use compile_check::*;
use compile_lower::*;
use compile_observation::*;
use compile_self_host::*;
use corpus::*;
use lsp_actions::*;
use lsp_completion::*;
use lsp_protocol::*;
use lsp_self_host::*;
use lsp_signature::*;
use lsp_symbols::*;
use package_dependency::*;
use package_init::*;
use package_lock::*;
use package_target::*;
use package_vendor::*;
use web_assets::*;
use web_package::*;
use web_types::*;

fn main() -> ExitCode {
    run_cli()
}

#[cfg(test)]
#[path = "tests/web_byte_buffer_abi.rs"]
mod web_byte_buffer_abi_tests;

#[cfg(test)]
#[path = "tests/python.rs"]
mod python_backend_cli_tests;

#[cfg(test)]
#[path = "tests/externs.rs"]
mod extern_package_tests;

#[cfg(test)]
#[path = "tests/service.rs"]
mod http_service_contract_tests;

#[cfg(test)]
#[path = "tests/migration_v57.rs"]
mod v57_migration_tests;

#[cfg(test)]
#[path = "tests/migration_v511.rs"]
mod v511_compatibility_tests;

#[cfg(test)]
#[path = "tests/migration_v512.rs"]
mod v512_compatibility_tests;

#[cfg(test)]
#[path = "tests/migration_v513.rs"]
mod v513_compatibility_tests;

#[cfg(test)]
#[path = "tests/migration_v517.rs"]
mod v517_compatibility_tests;

#[cfg(test)]
#[path = "tests/lsp_json.rs"]
mod lsp_json_tests;

#[cfg(test)]
#[path = "tests/vendor.rs"]
mod vendor_gate;
