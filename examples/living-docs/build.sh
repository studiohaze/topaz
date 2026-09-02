#!/usr/bin/env bash
# Build the Living Docs submission artifact from Topaz source. Run from the repository root:
#
#   ./examples/living-docs/build.sh
#
# Living Docs is a browser app: the Topaz interpreter compiled to WebAssembly runs both the
# Markdown renderer (examples/living-docs/render.tpz) AND each fenced `topaz` block. So the
# artifact is the WASM module + the static shell:
#
#   - playground/topaz_wasm/pkg/      the WASM module the editor loads (gitignored; built here)
#   - playground/living_docs/         the static shell (index.html) — serve + open in a browser
#
# It also builds the renderer as a native binary as a determinism check: `topaz run` and
# `topaz build` are held byte-identical by the differential suite, so the HTML the browser
# renders is what the native compiler produces (the in-browser block execution is the
# interpreter, also pinned run≡build against the native engine).
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

echo "==> toolchain"
cargo build --release -p topaz_cli

echo "==> WASM module (the browser app)"
wasm-pack build playground/topaz_wasm --target web --out-dir pkg

echo "==> renderer native binary (determinism check)"
rm -rf target/living-docs-native
./target/release/topaz build examples/living-docs/render.tpz --out-dir target/living-docs-native
( cd target/living-docs-native && cargo build --release )
mkdir -p dist
cp target/living-docs-native/target/release/program dist/living-docs-render
echo "    -> dist/living-docs-render (stdin Markdown -> stdout HTML, with compute-block placeholders)"

echo "==> done. Serve the repo root and open:"
echo "    http://localhost:8000/playground/living_docs/   (python3 -m http.server)"
