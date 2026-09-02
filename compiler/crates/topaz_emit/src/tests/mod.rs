//! Boxed-emitter tests partitioned by source-language feature family.
//! The shared fixture builders resolve and lower ordinary Topaz programs before
//! individual leaves inspect or execute emitted Rust.

use super::*;
use topaz_resolve::{InMemoryProvider, resolve, resolve_with_version};

fn unit_of(source: &str) -> LoweredUnit {
    let mut p = InMemoryProvider::new();
    p.add_file("main.tpz", source);
    topaz_lower::lower_resolved_compat(&resolve(&p, "main.tpz", None)).expect("test unit lowers")
}

fn unit_with_files(entry: &str, files: &[(&str, &str)]) -> LoweredUnit {
    let mut p = InMemoryProvider::new();
    for (path, source) in files {
        p.add_file(*path, *source);
    }
    topaz_lower::lower_resolved_compat(&resolve(&p, entry, None)).expect("test unit lowers")
}

fn unit_with_files_at(
    entry: &str,
    files: &[(&str, &str)],
    version: topaz_syntax::LangVersion,
) -> LoweredUnit {
    let mut provider = InMemoryProvider::new();
    for (path, source) in files {
        provider.add_file(*path, *source);
    }
    topaz_lower::lower_resolved_compat(&resolve_with_version(&provider, entry, None, version))
        .expect("test unit lowers")
}

mod calls;
mod control;
mod defaults;
mod expressions;
mod module;
mod patterns;
mod support;
