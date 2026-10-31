#!/bin/sh
set -eu
cd -- "$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
cargo build --release --locked --offline --jobs 1
target/release/topaz-seed --bootstrap . target/bootstrap-regenerated.rs
cmp generated/compiler.rs target/bootstrap-regenerated.rs
printf '%s\n' 'The compiler regenerated the same Rust source.'
