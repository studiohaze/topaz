#!/usr/bin/env bash
# Build the Markdown Live submission artifacts from the single Topaz source
# (examples/markdown-live/render.tpz). Run from the repository root.
#
#   ./examples/markdown-live/build.sh
#
# Produces:
#   - dist/markdown-live          a native binary: stdin (Markdown) -> stdout (HTML)
#   - playground/topaz_wasm/pkg/  the WASM module the browser editor loads
#
# Correctness note: `topaz run` (interpreter) and `topaz build` (native/emit) are held
# byte-identical by the differential test suite, so all three forms — interpreter, native
# binary, and the WASM playground runner — render the same HTML for the same input.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

renderer="examples/markdown-live/render.tpz"
echo "==> building the topaz toolchain"
cargo build --release -p topaz_cli

echo "==> native binary (topaz build -> cargo)"
rm -rf target/markdown-live-native
./target/release/topaz build "$renderer" --out-dir target/markdown-live-native
( cd target/markdown-live-native && cargo build --release )
mkdir -p dist
cp target/markdown-live-native/target/release/program dist/markdown-live
echo "    -> dist/markdown-live"

echo "==> WASM module (wasm-pack)"
wasm-pack build playground/topaz_wasm --target web --out-dir pkg
echo "    -> playground/topaz_wasm/pkg/ (open playground/markdown_live/index.html via a static server)"

echo "==> smoke test (native binary)"
printf '# It works\n\n- written in **Topaz**' | dist/markdown-live
