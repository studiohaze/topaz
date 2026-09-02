//! Generates the boxed Rust column for the Python differential fixture corpus.

mod build_support;
mod fixture_index;

use std::env;
use std::fs;
use std::path::Path;

use fixture_index::{MODULE_FIXTURES, SERVER_CONTRACT_DEMO, WIDE_FIXTURES};

const BUILD_INPUTS: &[&str] = &[
    "build.rs",
    "build_support/mod.rs",
    "build_support/model.rs",
    "build_support/render.rs",
    "fixture_index/mod.rs",
    "fixture_index/modules.rs",
    "fixture_index/wide.rs",
];

fn main() {
    for path in BUILD_INPUTS {
        println!("cargo:rerun-if-changed={path}");
    }
    for fixture in WIDE_FIXTURES {
        println!("cargo:rerun-if-changed={}", fixture.source_path);
    }
    for fixture in MODULE_FIXTURES
        .iter()
        .chain(std::iter::once(&SERVER_CONTRACT_DEMO))
    {
        for file in fixture.files {
            println!("cargo:rerun-if-changed={}", file.source_path);
        }
    }

    let generated = build_support::render::render();
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR");
    fs::write(Path::new(&out_dir).join("wide_core.rs"), generated).expect("write wide_core.rs");
}
