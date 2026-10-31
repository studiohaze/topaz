# Public upstream port record

The runtime and seed implementation in this branch was ported from the
development repository `clavef/topaz` at:

```text
9a89a6201ac541fb4d36c13498586c81cb7b0aba
```

That development repository is private. Access to it is not a public license
grant for all of its contents. This port uses the selected public seed scope
described by that repository's
`compiler/contracts/public-snapshot/seed/runtime.json`, and the seed runner
from the same source state.

## Ported paths

- `compiler/Cargo.toml`
- `compiler/rust-toolchain.toml`
- `compiler/generated/native/compiler.rs`
- `compiler/generated/native/compiler.json`
- `compiler/crates/topaz_diag/**`
- `compiler/crates/topaz_syntax/**`
- `compiler/crates/topaz_value/**`
- `compiler/crates/topaz_product_runtime/**`
- `compiler/crates/topaz_rt/**`
- `compiler/crates/topaz_host_none/**`
- `compiler/crates/topaz_host_native/**`
- `compiler/crates/topaz_seed_compiler/**`
- `compiler/crates/topaz_seed/**`

The six seed runtime crates are the approved public runtime closure. The
native host adapter is ported separately because it is the OS boundary needed
by native programs, not part of the no-capability seed compiler.

## Deliberately not ported

The full `topaz_cli` workspace, Rust reference compiler, Lispex evaluator,
MCP services, private product assets, and generated program images are not
inputs to this Core Kit path. Their omission is intentional and keeps command
policy in Topaz rather than importing the website product.

The canonical Topaz source under
`compiler/crates/topaz_self_frontend/topaz/` was already present in this
repository and matches the source files named by
`compiler/generated/native/compiler.json`. `core-kit/verify-core.ps1` and
`core-kit/verify-core.sh` verify that source inventory independently.

## Verification state

Earlier local work reported a standalone seed build and native host tests.
Those reports do not verify the later Core Kit integration. The complete core
request is now committed under `compiler/seed/initial`, and the CLI has its own
separate input under `core-kit/cli/initial`. Subsequent Linux execution covered
bootstrap, the Topaz CLI and a generated native sample; independent complete
development executables also matched under the recorded build recipe. The
owner reported successful Windows bootstrap, native smoke and byte-identical
canonical core regeneration on the earlier integration revision. Exact
platform/revision boundaries and remaining gates are recorded in
[VALIDATION.md](VALIDATION.md) and [STATUS.md](STATUS.md).
The new Core Kit adapter, generator and Topaz commands are local additions,
not an import of the private full CLI. No new assertion about the entire
development repository's licensing is made here.
