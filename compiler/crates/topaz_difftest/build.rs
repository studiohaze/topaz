//! Generates the differential-harness fixture module (CDR-006 §7). For
//! each eligible fixture it resolves the source and emits the program
//! as `emit_module` output, wrapped in its own `mod fixture_N`, plus a
//! `FIXTURES` table of `(name, source, run_fn)`. The generated source
//! is `include!`d by the crate, so the emitted programs compile AS PART
//! of the workspace (one type universe) — a green build is the
//! compile-shape proof, and the test then runs each program through
//! both engines and compares.
//!
//! Build-script types never cross into the runtime crate: only Rust
//! SOURCE and the fixture source BYTES are emitted; the test re-resolves
//! independently with the crate's own dependencies.

mod build_support;
mod fixture_index;

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for fixture in fixture_index::FIXTURES {
        println!("cargo:rerun-if-changed={}", fixture.source_path);
    }

    let generated = build_support::render::render_fixtures();
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR set by cargo");
    fs::write(Path::new(&out_dir).join("fixtures.rs"), generated)
        .expect("write generated fixtures");
}
