# Core Kit implementation

Core Kit combines the canonical Topaz compiler, a CLI written in Topaz and the
public native runtime. This directory contains the command implementation,
its initial compile inputs, native connection code and developer workflows.
The [root README](../README.md) introduces the product and its distribution.

## Implementation boundaries

| Location | Responsibility |
| --- | --- |
| [`cli/src/`](cli/src) | Commands, projects, compiler requests, diagnostics, builds, maintenance and payload policy in Topaz |
| [`cli/initial/request.json`](cli/initial/request.json) | Complete initial input for compiling the CLI |
| [`cli/REQUEST.json`](cli/REQUEST.json) | Explicit source and standard-module mappings |
| [`cli/std/`](cli/std) | Canonical seed exports and additional public standard-module boundary declarations |
| [`cli/templates/`](cli/templates) | Native program entry, default capabilities and public runtime lockfile |
| [`native/src/lib.rs`](native/src/lib.rs) | Canonical compiler callback and source-provider relay |
| [`native/src/process.rs`](native/src/process.rs) | Child processes, terminal I/O and bounded output capture |
| [`native/src/platform.rs`](native/src/platform.rs) | Rooted path inspection, directory and file operations, file modes and target metadata |
| [`native/src/main.rs`](native/src/main.rs) | Native entry point and exit or error handling |
| [`native/src/bin/generate-cli.rs`](native/src/bin/generate-cli.rs) | Mechanical initial-input materialization for a source bootstrap |
| [`../compiler/core-kit-native/Cargo.toml`](../compiler/core-kit-native/Cargo.toml) | Native package manifest inside the public Cargo workspace |
| [`repro-build.sh`](repro-build.sh) | POSIX bootstrap with recorded source-path mappings |
| [`verify-core.data`](verify-core.data) | Recorded source, seed and CLI byte inventory |
| [`distribution/`](distribution) | Versioned platform recipes, component records, notices and runtime companions |
| [`downloads.data`](downloads.data) | Pinned optional GitHub Release assets and exact input identities |
| [`../install-core-kit`](../install-core-kit) and [`../install-core-kit.cmd`](../install-core-kit.cmd) | First-run OS adapters that verify archive and executable bytes before calling Topaz |

The CLI export takes `args`, `repositoryRoot`, `compile`, `process`, `native`
and `capture`. The canonical compile callback accepts a JSON request and a
Topaz source provider. The core runs with no ambient filesystem capabilities,
while provider calls return to the original CLI context.

Topaz reads `topaz.toml`, answers the compiler's source queries, chooses native
build arguments and checks the installed toolchain. Rust performs the actual
OS operations and callable transport through the public runtime and canonical
compiler interfaces. Project resolution and command policy live in the Topaz
modules.

## Command paths

The [command guide](cli/README.md) covers source checks, Rust emission, source
and project snapshots, native program builds, Kit builds, source maintenance,
native image inspection, payload assembly and release packaging, and local
installation of a pinned archive.

Project mode supports one package, its local module graph and the bundled
standard modules. A saved project envelope also retains filesystem grants.
See [PROJECTS.md](PROJECTS.md) for the package contract, admitted manifest and
path rules.

Use `kit build` when a Core Kit executable is already available and you want
to compile the current CLI sources. It performs the compile and Cargo-link
sequence in Topaz and writes a local Kit under `out/core-kit-build`.

Use `kit refresh --check` to inspect the current CLI and its recorded inputs
without writing files. Use `kit refresh` after a source-set change to update
the mappings, initial snapshot and inventories together. The command checks
the actual native callable contract, preserves protected core inputs and
retains backups before replacing metadata.

Use the repository's `bootstrap` or `bootstrap.cmd` when starting from
Rust/Cargo without running a prebuilt Kit. This path builds the public core
and mechanical generator before compiling and linking the CLI. Both paths
reuse `compiler/target`, preserve request and response files, and leave the
expensive canonical self-regeneration separate.

## Optional platform downloads

Build this checkout using the [source bootstrap](../README.md#build-the-kit-from-source).
When `downloads.data` lists a GitHub Release asset for your platform,
`install-core-kit` or `install-core-kit.cmd` can fetch the matching archive.
Pass `--archive` to use an already downloaded copy.

The OS adapter checks archive and executable bytes before the executable's
first run. Topaz's `kit install` then validates the full archive, assembled
payload and current source input identity. It owns the installation lock,
staging and final version-directory activation. Installed versions stay under
`out/core-kit-installed`, and the ordinary launchers use the exact catalogue
selection without network access.

Source bootstrap remains independent of that installation path. It writes
`out/core-kit-bootstrap` and reports an executable to run directly. It does
not install a release or replace an installed version.

## Prepare platform payloads

Use `kit payload-assemble` with a completed Kit build and a checked-in recipe
under `distribution/`. Topaz admits the request and successful compiler
response, checks the executable's format and declared dependencies, and
prepares its generated CLI, component records, notices and runtime files in
a new directory. The candidate includes `assembly.json` and `payload.json`.

`kit payload-compare` checks two assembled candidates, including all file
bytes, manifests and required modes. Keep their build records to establish
how they were produced. The comparison itself does not demonstrate independent
construction.

`kit payload-promote` requires a recipe approved for release and a Kit running
on the candidate target. It compares the candidates, runs the selected
executable checks and copies the same bytes to the new `--out-dir` staging
directory. It retains the manifest and refuses an existing destination.

`kit payload-pack` applies the same native approval gate and prepares the
named release archive plus its download catalogue row. Topaz writes and
checks the archive's deterministic representation. The publisher reviews the
outputs, uploads the asset to the declared GitHub Release and commits the
catalogue update. Packaging does not publish or modify Git.

Use `kit binary-info --file` to inspect a native image without executing it.
Add `--other` for a bounded comparison of two images. Structural information
helps investigate a mismatch. Target support and minimum system requirements
come from the recipe and recorded native validation.

## Verification scope

| Check | Scope |
| --- | --- |
| `kit verify` | Recorded source, initial-source, manifest and CLI input bytes |
| `kit refresh --check` | Current CLI type and callable contract, mappings, initial snapshot and inventories |
| `kit payload-verify` | Exact candidate files and declared checkout inputs |
| `kit payload-compare` | Complete assembled candidate bytes, manifests and required modes |
| `kit payload-pack` | Native approval, canonical archive bytes and generated download record |
| `kit install --archive` | Pinned archive, assembled payload and current checkout input identity |
| `kit install --check` | Selected installation, reconstructed canonical archive identity and current checkout inputs |
| `bootstrap --verify-core` | Canonical compiler regeneration and generated-Rust comparison |
| Independent payload reproduction | Complete native outputs and accompanying files under a recorded environment |

PowerShell verification adds parsed relationships between the JSON inputs to
the common byte checks. A consistent inventory does not establish a compiler
rebuild, and a core regeneration result does not establish complete native
payload reproduction.

## Developer guides

- [Source bootstrap and Kit builds](BUILD.md)
- [Project manifests and saved inputs](PROJECTS.md)
- [CLI snapshot and inventory maintenance](MAINTENANCE.md)
- [Local execution handoff](LOCAL-VALIDATION.md)
- [Executed validation and artifact identities](VALIDATION.md)
- [Independent native reproduction](REPRODUCIBILITY.md)
- [Payload assembly and publication](PAYLOADS.md)
- [Implementation and release status](STATUS.md)

Each distributed target carries its platform requirements and recipe identity
in `assembly.json`. The validation record identifies the source revisions,
build environments and comparisons behind the published files. Consult those
records when reproducing a target or investigating a platform-specific result.
