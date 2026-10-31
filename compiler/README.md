# Public Topaz compiler and Core Kit workspace

This is the reduced public runtime/bootstrap workspace, based on the approved
seed components. It includes the canonical generated compiler, its standalone
seed runner, the public native host, and the Core Kit native package whose
manifest is at `core-kit-native/Cargo.toml`. The adapter Rust source remains
under `../core-kit/native`; the package manifest is inside this workspace so
its Cargo identity does not depend on the absolute checkout location.

The six original seed runtime crates are `topaz_diag`, `topaz_syntax`,
`topaz_value`, `topaz_product_runtime`, `topaz_rt` and `topaz_host_none`.
`topaz_host_native` supplies native filesystem/console effects. The
`topaz_seed_compiler` crate includes the unchanged generated compiler Rust.
The standalone `topaz_seed` runner uses the no-capability host.

## Build the Kit from source

From the repository root, use `./bootstrap` or `.\bootstrap.cmd` with an
already-installed Rust/Cargo 1.96.0 and native linker. The scripts check the
installed tools, disable automatic toolchain installation, choose the native
host target and use the checked-in lockfile with frozen/offline resolution.
See [the full sequence](../core-kit/BUILD.md).

The workspace's `topaz-core-kit-generate-cli` target compiles independently of
any generated CLI output. It reads the dedicated CLI input/mappings, fills
current source bytes and invokes the canonical `compile` export directly.
The `native-cli` feature then links the generated CLI and public core into
`topaz-core-kit`. The product calls Topaz's `run` export and supplies explicit
compiler/process callbacks. Command policy lives in Topaz.

`seed/initial` is the complete input for **compiler self-regeneration**.
`../core-kit/cli/initial` is the separate complete **CLI compilation** input.
They must not be combined by appending another entry fact. Compiler
self-regeneration is only requested by `bootstrap --verify-core`.

No website `topaz_cli`, Rust reference compiler or private Lispex evaluator is
a dependency of this workspace. The existing core source/generated Rust and
runtime ABI are not changed by the Core Kit adapter.

## Reconstruction and verification

The canonical generated compiler, public runtime, compiler self-regeneration
input and separate CLI input are retained together in this workspace. Source
bootstrap starts from the installed Rust toolchain and these local materials.
It does not need a release download or an already installed Core Kit.

The bootstrap reports a new executable under the selected output directory.
Run it by that path. Download installation and catalogue selection are separate
operations described in the [root README](../README.md).

Use [VALIDATION.md](../core-kit/VALIDATION.md) for the exact revisions,
environments and comparisons behind observed results, and
[STATUS.md](../core-kit/STATUS.md) for implementation and release readiness.
The [local validation guide](../core-kit/LOCAL-VALIDATION.md) explains how to
preserve native artifacts and investigate a mismatch without repeating
unrelated compiler regeneration.
