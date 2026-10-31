# Core Kit commands

The command implementation lives in `src/`. Topaz handles options, project
configuration, compiler requests, diagnostics, filesystem grants, verification,
Cargo builds, source maintenance, release archives and local installation. The native adapter
supplies the operating-system and callable connections those commands use.

The exported `run` function takes six arguments, in this order.

| Argument | Purpose |
| --- | --- |
| `args` | The original command arguments |
| `repositoryRoot` | The selected Core Kit data checkout |
| `compile` | Call the canonical compiler with request JSON and a Topaz source-provider closure |
| `process` | Run an executable with an argument array, working directory and environment, inheriting its streams |
| `native` | Inspect paths and file metadata, create and move directories, copy files, apply admitted file modes and report the Kit's build target |
| `capture` | Run a metadata command and return its exit code, stdout and stderr |

The compiler runs with a capability-free host. Its source-provider callback
returns to the original CLI context, where Topaz supplies requested project
facts. Rust transports values across that boundary and does not parse a
project manifest or discover its module graph. The CLI uses the public runtime
and the canonical compiler provided in the checkout.

## Commands

The examples below use `topaz-core-kit` as the executable name. Use the
repository launcher or the path of your locally built executable as needed.

```text
topaz-core-kit help
topaz-core-kit --version
topaz-core-kit hash data/input.txt
topaz-core-kit kit binary-info --file out/core-kit-bootstrap/topaz-core-kit
topaz-core-kit kit binary-info --file out/core-kit-bootstrap/topaz-core-kit --other out/core-kit-build/topaz-core-kit
topaz-core-kit check --root examples/core-kit/project
topaz-core-kit emit-rust --root examples/core-kit/project --out out/project.rs
topaz-core-kit request --root examples/core-kit/project --out out/project.request.json
topaz-core-kit build --root examples/core-kit/project --out-dir out/project-native
topaz-core-kit check --source examples/core-kit/hello.tpz
topaz-core-kit emit-rust --source examples/core-kit/hello.tpz --out out/hello.rs
topaz-core-kit request --source examples/core-kit/hello.tpz --out out/hello.request.json
topaz-core-kit check --request out/project.request.json
topaz-core-kit build --request out/project.request.json --out-dir out/replayed-project
topaz-core-kit kit build
topaz-core-kit kit build --out-dir out/my-kit
topaz-core-kit kit build-id
topaz-core-kit kit verify
topaz-core-kit kit refresh --check
topaz-core-kit kit refresh
topaz-core-kit kit payload-assemble --build-dir <build-directory> --recipe <repository-relative-recipe> --out-dir <new-candidate-directory>
topaz-core-kit kit payload-compare --payload-dir <first-candidate> --other <second-candidate>
topaz-core-kit kit payload-promote --payload-dir <first-candidate> --other <second-candidate> --out-dir <new-stage>
topaz-core-kit kit payload-pack --payload-dir <first-candidate> --other <second-candidate> --out-dir <new-release> --repository studiohaze/topaz --tag <release-tag> --source-commit <40-character-commit>
topaz-core-kit kit install --archive <matching-archive>
topaz-core-kit kit install --check
topaz-core-kit kit payload-record --target x86_64-unknown-linux-gnu --payload-dir out/payload-candidate
topaz-core-kit kit payload-verify --payload-dir out/payload-candidate
```

`check`, `emit-rust` and `build` require exactly one of `--root`, `--source`
and `--request`. The `request` command creates a snapshot from either `--root`
or `--source`. Emission and request commands require `--out`, while native
program builds require `--out-dir`.

Paths are relative to the caller's working directory. Emission and request
commands create missing parent directories after their input is admitted and
any required compilation succeeds. Native program and Kit builds create their
output directory and accept an empty directory or one marked by the same kind
of earlier build. They refuse to overwrite an unrelated project or use an
output containing symlink entries.

Supply `--recipe` relative to the checkout root, including the
`core-kit/distribution/` directory.
The CLI accepts either slash separator and records forward slashes. Payload
assembly, promotion and packaging require a new `--out-dir`. The catalogue
selects the installed version's location inside the checkout.

`kit build-id` prints the SHA-256 of the generated CLI Rust selected when the
running Kit was linked. Before promoting or packaging a new candidate, the Kit
compares this output with the digest of the candidate's `generated_cli.rs`.
If the native link failed in a reused output directory, the source records can
be newer than the EXE. Rebuild in a fresh directory before retrying. Older
executables can still be inspected and existing pinned archives remain
verifiable. New promotions and packages require a candidate with `kit build-id`.

Compiler diagnostics include their code, module index and byte range. A
rejected compile returns a failure and is not written as generated Rust or
passed to Cargo. The native boundary prints returned errors on stderr.

## Project and source inputs

`--root` reads `topaz.toml` for one package and resolves local module imports
through the canonical compiler's existing fact-provider interface. The Topaz
provider supplies source text, directory entries and physical containment
information as the compiler requests them. It does not parse import syntax.
The bundled project uses two source modules and needs no additional download.

The supported package dependency is the bundled `std` version. Other package
dependencies, extern replay and private evaluator requests are outside this
project contract and return an explicit error. The
[project guide](../PROJECTS.md) documents the manifest, source limits,
containment rules and filesystem grants.

`--source` creates a complete virtual one-file request with the bundled
`std.bytes`, `std.fs`, `std.io`, `std.json` and `std.toml` boundary modules.
It does not read a project manifest. Use `--root` for local imports or supply
a complete saved request. Source text and the bundled standard-module text
together are limited to 16 MiB in source mode.

`--request` accepts two saved input formats.

| Format | Contents and behavior |
| --- | --- |
| `topaz.compiler.stage1-exchange/v2` | A complete public compiler request with source facts and standard modules |
| `topaz.core-kit.project-request/v1` | A public compiler request together with the project configuration and filesystem grants |

Both paths enforce the canonical source-set identity and resource budgets and
reject duplicate facts or modules, incomplete entry facts and extern replay.
The command selects `typed` for checking and `rust-source` for emission or
native builds. Saved inputs do not read missing facts from the ambient
filesystem.

`request --root` asks the compiler to discover and check the module graph, then
saves the collected facts in deterministic order with the project configuration.
It records logical paths rather than the project's absolute location. The
original project directory is not required for replay. Preserve this project
envelope when native rebuilds need the same filesystem grants. A raw Stage 1
request carries no project grant configuration.

## Native program builds

`src/native_build.tpz` writes the generated Rust, native entry template,
capability configuration, Cargo manifest and fixed public runtime lockfile
into the selected output directory. It calls Cargo through the process
callback with an explicit argument array, working directory and environment.

The shared Topaz toolchain policy verifies installed Rust/Cargo 1.96.0 and
checks that Rust's host matches the Kit's native target. It sets
`RUSTUP_AUTO_INSTALL=0`, Cargo offline mode and `--frozen`, then selects the
explicit target. The toolchain and platform linker or SDK must already exist.
A successful command reports the complete native executable path and does
not run the program automatically.

Project builds generate a host using `NativeHost::with_fs_capabilities`.
Read and write roots remain separate. An omitted grant is an empty grant,
including a project with no filesystem declaration. Grant paths are relative
to the application's working directory when launched, so the binary does not
embed the build machine's absolute project path. The saved project envelope
retains those grants for later builds.

Standalone `--source` builds and raw Stage 1 requests use ordinary process
filesystem access. Their native adapter uses the public runtime stack contract
and propagates the generated main function's exit code or error.

## Rebuild the Kit

`kit build` is implemented in Topaz. It reads current CLI files through
`REQUEST.json`, prepares the closed CLI input, invokes the canonical compile
callback and links the resulting Rust into a new Kit. It does not invoke a
bootstrap shell script or regenerate the compiler core.

The default output is `out/core-kit-build` under the selected checkout. An
explicit `--out-dir` is relative to the caller. The command retains
`cli.request.json`, `cli.response.json` and `generated_cli.rs` alongside the
new executable, and reuses the checkout's `compiler/target` cache.

Before compiling, `kit build` checks the generated core against its canonical
manifest and validates the CLI source and directory mappings. It deliberately
reads current mapped source bytes so a local edit can be built. Keep the
committed initial snapshot and verification inventories consistent before
publishing the source change.

The Topaz toolchain policy chooses the version, host target, Cargo arguments
and environment. The native adapter captures tool metadata, creates the
selected output directory and copies the resulting executable while retaining
its permissions. A local Kit build does not replace or activate an installed
release. Invoke its reported executable path. See [BUILD.md](../BUILD.md) for
the separate source bootstrap that starts
without running a prebuilt Kit.

## Input verification

`kit verify` uses the same versioned `core-kit/verify-core.data` inventory as
the platform scripts. It validates record kinds, field counts, unique keys,
declared totals, required inputs, relative paths, lowercase SHA-256 values
and canonical compiler identity before reading inventory-selected files.
Absolute paths, drive prefixes, backslashes, empty segments and traversal
are rejected.

The inventory covers canonical source pairs, the original public snapshot,
generated core Rust, manifests, source requests, standard modules and the
recorded CLI inputs. JSON inputs are compared as exact bytes. This checks
consistency with the recorded inventory. It does not perform a rebuild or
authenticate the inventory's publisher.

## Native image inspection

`kit binary-info --file <file>` reads a native image without executing it. It
reports the file's size and hash, admitted image profile, sections, declared
loader dependencies and available structural metadata. Add `--other <file>`
for a bounded comparison of two images. This helps locate changed regions
without printing a complete byte-by-byte difference of a large executable.

The command applies bounds to image size, offsets, tables and strings, and
returns a diagnostic for malformed or unsupported inputs. Structural image
information does not identify the complete build environment, prove native
compatibility or replace a platform execution check. Minimum system
requirements belong to the target recipe and its recorded validation.

## Assemble a platform payload

`kit payload-assemble` takes a completed bootstrap or Kit-build directory, a
checked-in platform recipe and a new output directory.

```text
topaz-core-kit kit payload-assemble --build-dir <build-directory> --recipe <repository-relative-recipe> --out-dir <new-candidate-directory>
```

The build directory supplies `cli.request.json`, `cli.response.json`,
`generated_cli.rs` and the native executable. Assembly compares the recorded
request with the current CLI inputs, admits the successful compiler response
and requires its Rust output to match the generated file. It also checks the
native image's format, target profile and declared runtime dependencies
against the selected recipe.

Recipes live under `core-kit/distribution/` and are included in the repository
input list. They specify the executable name, platform requirements, allowed
system dependencies and exact companion-file mappings. Thirteen baseline
notice and component files are required. Additional notices and runtime files
come from the recipe's declared repository inputs. These records retain their
existing component scopes and terms.

Every declared loader dependency must be classified by the recipe as a system
dependency or a bundled companion. The recipe does not grant general loader
access or establish the compatibility of every library with every host.
Native platform validation supplies that evidence.

Assembly admits the input set before creating its destination. It copies the
executable, generated CLI Rust and required companions into a fresh directory,
applies the prescribed file modes, then writes `assembly.json` and
`payload.json`. It verifies the result before reporting success. An existing
output directory is refused. Candidate membership excludes symlink entries
and unexpected files. Each assembled file is limited to 256 MiB. Packaging
also limits the complete raw archive to 256 MiB, including headers and padding.

`assembly.json` identifies the recipe and its hash, the compiler input and
generated Rust identities, native image profile, classified system
dependencies, platform requirements and expected file roles and modes. Its
evidence kind is `assembled-candidate`. `payload.json` supplies the separate
exact byte inventory. Assembly records the checked relationships and copies
the supplied executable. It does not establish that executable's independent
build history.

## Compare and promote a payload

```text
topaz-core-kit kit payload-compare --payload-dir <first-candidate> --other <second-candidate>
topaz-core-kit kit payload-promote --payload-dir <first-candidate> --other <second-candidate> --out-dir <new-stage>
```

Comparison verifies each candidate against the current repository and its
checked-in recipe, then compares all file bytes, both manifests and required
modes. It requires two distinct directories. Use a host and filesystem that
can validate the recipe's modes. Windows does not report POSIX permission
bits and cannot validate a required Unix executable mode.

Two equal candidates demonstrate equal outputs. Retain the records of their
separate builds to establish independent construction, including toolchain,
linker, SDK, environment and profile. Directory names and matching manifests
alone do not supply that history.

Promotion requires `releaseReady` in the checked-in recipe and a running Kit
whose native target matches the candidate. It compares the candidates and
runs the selected executable's version, source check, project check and
input-verification commands. The candidates are verified again after these
execution checks.

Promotion requires a new `--out-dir` staging directory. It copies the
compared bytes without rebuilding the executable or regenerating its manifest,
retains the required modes and copies the executable last. The resulting
directory is compared with the candidate again before success is reported.

A staged directory is not installed automatically. Promotion does not modify
Git, create a release or replace an installed version. A development recipe
can assemble and compare candidates, while promotion and packaging require
an approved release recipe.

## Package a named release

```text
topaz-core-kit kit payload-pack --payload-dir <first-candidate> --other <second-candidate> --out-dir <new-release> --repository studiohaze/topaz --tag <release-tag> --source-commit <40-character-commit>
```

Packaging applies the complete comparison and native approval gate to both
candidates. It writes `topaz-core-kit-<target>.tar.gz`, a `downloads.data`
record and a `package.json` receipt into a new output directory. Topaz
constructs and checks the archive and records its exact bytes and the
executable's bytes. No external packager,
Python, Node or jq is required.

The archive has a deterministic flat layout, file ordering, metadata and
representation. Its gzip stream uses stored DEFLATE blocks supplied by the
public runtime. This fixes the encoded bytes without promising a smaller
download than the raw files. Files and the full raw tar are limited to
256 MiB. Received archive bytes are bounded separately at 512 MiB.

The row binds the target, named GitHub Release, source commit declaration,
actual repository input identity, archive and executable. The input identity
is the SHA-256 of the canonical JSON representation of the sorted repository
file records, without an appended LF. `core-kit/downloads.data` is excluded
from that inventory to avoid a self-reference. Installer and launcher sources
remain included. The declared commit is provenance and does not substitute
for checking the actual input identity.

Publish the resulting archive under the exact repository, tag and asset name
and review the generated catalogue row as a separate source change. The
command makes no network request, Git change or release upload. See
[PAYLOADS.md](../PAYLOADS.md) for the publication checks.

## Install a pinned archive

For a first installation, use the repository's `install-core-kit` or
`install-core-kit.cmd`. Supply `--archive <file>` for an offline installation
of the catalogue's exact archive. These small OS adapters select the native
target and check archive length and SHA-256 before extracting only its fixed
executable member. They check that executable's length and SHA-256 before
calling it.

The actual installation policy is Topaz code.

```text
topaz-core-kit kit install --archive <matching-archive>
```

The command admits the strict catalogue, requires the running Kit's native
target, compares the current repository input identity and verifies the
archive's canonical representation and complete assembled payload. It creates
and validates a separate stage, then activates a new version directory under
`out/core-kit-installed/<target>/<archive-sha256>/`. Its `payload/` contains
the files and its sibling `installation.data` records the selection. Topaz
owns the lock and final directory rename. An existing matching installation
is rechecked and retained. Full installed-version verification re-encodes the
complete payload into its canonical archive and compares its length and
SHA-256 with the catalogue. A rewritten local manifest cannot substitute a
different companion set for the pinned archive. Failed admission does not
replace an older version.

An empty catalogue or a missing target returns source-bootstrap guidance.
The OS adapters do not request a different release or run an unverified
executable. The root launchers use the exact selected local version without
downloading, updating, building or revalidating the whole source tree. The
installed Kit can therefore maintain and build edited CLI sources.
`kit install --check` explicitly verifies the installation and its current
checkout inputs, including that complete archive identity, without installing
a version. Archive reconstruction runs during these explicit installation
operations and does not add work to ordinary launcher invocation. A source
bootstrap remains a
separate executable at its reported output path.

## Payload inventories

`kit payload-record` writes `payload.json` beside an assembled candidate.
`kit payload-verify` compares the record with every candidate file and the
selected checkout's required inputs. These Topaz commands do not compile,
run or assemble the candidate.

`kit payload-assemble` records its candidate automatically. For a separate
inventory exercise, prepare a directory and copy its executable and companion
files before recording. The executable name is `topaz-core-kit.exe` for Windows
and `topaz-core-kit` for the other mapped targets. Include the applicable
notices and build records, and preserve POSIX executable permissions.
The inventory accepts a flat directory of regular files and rejects symlinks,
subdirectories, case aliases, Windows reserved names and nonportable filenames.

Each record includes the relative name, byte count and SHA-256 of its payload
files and the paths listed in [payload-inputs.data](../payload-inputs.data).
That sorted checkout list includes itself and the installer sources. It
excludes only the release catalogue from otherwise selected distribution
inputs, preventing a cycle between archive and catalogue hashes. Verification
collects required
paths independently of the manifest, so omitting a manifest row cannot remove
a required check. Both payload commands also require the recorded-input
checks to pass.

Recording refuses to replace an existing `payload.json`. Verification requires
the exact machine-written JSON representation and final LF. Extra or missing
files, changed bytes, unknown or duplicate fields and reordered records fail.
There are no timestamps or absolute paths in the manifest, so equal candidate
bytes and target values produce the same record in different directories.

The manifest identifies its evidence as `recorded-inputs-only`. It checks
file consistency without proving an independent build, platform ABI,
executable permissions, publisher identity or complete licensing. Keep files
unchanged while recording or checking them. Publication requires the separate
platform and reproduction work described in [PAYLOADS.md](../PAYLOADS.md).

## Initial input and source maintenance

The descriptor lists the CLI's explicit source and standard-module mappings.
The separate initial request is a complete input for compiling the CLI through
the public core. Both ordinary bootstrap and kit build read the current mapped
files, so an existing module can be edited before regenerating the recorded
snapshot.

Use `kit refresh --check` to type-check the current CLI and report stale
metadata without writing files. Use `kit refresh` to discover added or removed
modules, rebuild the initial request and update the verification and payload
path inventories. The command preserves the compiler contract and refuses
changed protected core or seed inputs. It records backups before replacing
its four managed metadata files and does not run Cargo or core regeneration.

[MAINTENANCE.md](../MAINTENANCE.md) describes managed paths, limits, the retained
receipt and recovery after an interrupted update. Keep the resulting metadata
with the source change in Git. Dependencies outside the managed trees still
require an explicit curated inventory update and component review.

## Module responsibilities

| Module | Responsibility |
| --- | --- |
| `src/main.tpz` | Six-argument callable entry point |
| `src/arguments.tpz` | Command and option parsing |
| `src/contracts.tpz` | Exchange shapes and callback types |
| `src/requests.tpz` | Single-source requests and closed-input admission |
| `src/project_manifest.tpz` | TOML configuration and filesystem grant validation |
| `src/project_sources.tpz` | Requested source, directory and containment facts |
| `src/project.tpz` | Project preparation and saved project envelopes |
| `src/responses.tpz` | Compiler response projection and diagnostics |
| `src/commands.tpz` | Command sequencing and output policy |
| `src/toolchain.tpz` | Installed Rust/Cargo version and host checks |
| `src/native_build.tpz` | Native program files, grants and Cargo policy |
| `src/kit_inputs.tpz` | Current CLI input mapping and canonical core admission |
| `src/kit_build.tpz` | CLI compilation and Kit linking |
| `src/verification.tpz` | Input inventory validation and SHA-256 comparison |
| `src/refresh.tpz` | Maintenance orchestration, type checking, backups and rollback |
| `src/refresh_inputs.tpz` | Strict descriptor admission and current CLI source facts |
| `src/refresh_inventory.tpz` | Protected input checks and managed inventory preparation |
| `src/payload.tpz` | Payload recording and comparison |
| `src/payload_files.tpz` | Portable file lists and declared checkout inputs |
| `src/release_image.tpz` | Bounded native image parsing, sections, loader declarations and metadata |
| `src/binary_info.tpz` | Native image reports and bounded comparisons |
| `src/release_recipe.tpz` | Platform recipe admission and required companion mappings |
| `src/release_payload.tpz` | Candidate assembly, complete comparison and native approval policy |
| `src/download_catalog.tpz` | Strict pinned release rows and repository input identity |
| `src/release_archive.tpz` | Canonical flat archive writer and admission |
| `src/release_package.tpz` | Approved release archive and catalogue generation |
| `src/installation.tpz` | Pinned archive validation, installation lock, staging and activation |

The platform smoke scripts cover the source and project examples, saved-input
replay, compiler failures, input and payload verification and native samples.
[VALIDATION.md](../VALIDATION.md) records which revisions and environments have
actually run these checks.
