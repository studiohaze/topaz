# Core Kit source build and verification

## Host environment

Provide the installed Rust/Cargo version pinned in
[`compiler/rust-toolchain.toml`](../compiler/rust-toolchain.toml), the host's
native linker and SDK, and the ordinary OS command tools. The current pin is
Rust/Cargo 1.96.0. Core Kit disables implicit Rust toolchain installation and
uses frozen, offline Cargo resolution against the local dependency graph.
Python, Node, jq, a website Topaz executable and extra artifact downloads are
not part of the build path.

Builds use the native target explicitly. The Topaz build commands read Rust
and Cargo metadata, verify their versions and require Rust's host to match
the running Kit's target. These decisions live in `cli/src/toolchain.tpz`.
The native adapter only captures the subprocess output and reports the target
compiled into the Kit.

[VALIDATION.md](VALIDATION.md) records the configurations that have actually
run, including development profile overrides. Keep an override with its
result rather than treating that result as a default-profile release.

## Rebuild with an existing Kit

From the repository root, a locally bootstrapped Kit can rebuild the current
CLI through its Topaz implementation.

```sh
./out/core-kit-bootstrap/topaz-core-kit kit build
./out/core-kit-build/topaz-core-kit --version
```

```powershell
.\out\core-kit-bootstrap\topaz-core-kit.exe kit build
.\out\core-kit-build\topaz-core-kit.exe --version
```

The root `topaz` launchers can invoke the same `kit build` command when a
downloaded installation is selected. Use `kit build --out-dir out/my-kit` for a custom
output. Explicit relative paths are relative to the caller, while the default
`out/core-kit-build` belongs to the selected checkout.

The command performs these steps in Topaz.

1. Check the installed Rust/Cargo version and host target.
2. Check the canonical generated core against its manifest and prepare the
   CLI request from current explicit file mappings.
3. Create or admit the output directory and retain `cli.request.json`.
4. Invoke the canonical compiler and retain `cli.response.json` and the
   successfully generated `generated_cli.rs`.
5. Ask Cargo to link the native Kit using that generated source and the
   existing public runtime workspace.
6. Copy the resulting executable into the selected output and report its path.

This path does not run the bootstrap script, a second compiler implementation
or the expensive canonical core regeneration. Topaz chooses the files,
arguments, environment and failure policy. The native callbacks perform the
requested process and filesystem operations.

The output must be empty or marked by a previous `kit build`. An unrelated
project or a symlink entry is refused. The command preserves downloaded
versions under `out/core-kit-installed` and writes local results separately. An executable copied outside its
checkout needs `TOPAZ_CORE_KIT_ROOT` to select the matching data checkout.

## Bootstrap without a prebuilt Kit

Use the root bootstrap when no Core Kit executable is available or when you
want to start without downloading or running a prebuilt native binary.

```sh
./bootstrap
```

```powershell
.\bootstrap.cmd
```

A custom bootstrap output uses `--out`.

```sh
./bootstrap --out out/my-bootstrap
```

```powershell
.\bootstrap.cmd --out out\my-bootstrap
```

The bootstrap follows a fixed sequence.

1. Select the installed pinned toolchain, disable implicit installation and
   determine the host target.
2. Build `topaz-core-kit-generate-cli` in the `compiler` workspace with release,
   frozen resolution, an explicit target and `compiler/target` as its cache.
3. Materialize the mapped CLI input and invoke the canonical core's compile
   export to generate the CLI Rust.
4. Retain the request, response and generated Rust in the output directory.
5. Build `topaz-core-kit` with the `native-cli` feature and
   `TOPAZ_CORE_KIT_GENERATED_CLI` pointing to that output.
6. Copy the native executable into the output directory.

The checked-in canonical generated Rust and installed Rust toolchain are the
starting point. The website CLI, reference compiler, private evaluator and
downloaded Core Kit executable do not participate.

The resulting executable remains under the selected bootstrap output. Run it
by its reported path. Bootstrap does not install a release, create an
installation receipt or change the root launcher's catalogue selection.

Windows uses its native `bootstrap.cmd`, without Git Bash or an execution-policy
change. The optional `core-kit/bootstrap.ps1` targets Windows PowerShell 5.1
and newer in environments where scripts are already allowed.

## Shared native build layout

The native package manifest is `compiler/core-kit-native/Cargo.toml`, inside
the same Cargo workspace as the public core. Its Rust sources remain in
`core-kit/native`. Keeping the package inside the workspace avoids an absolute
checkout path in Cargo's package identity. Source-path remapping remains a
separate part of the reproduction recipe.

With `native-cli` enabled, `core-kit/native/build.rs` reads the selected
`TOPAZ_CORE_KIT_GENERATED_CLI` file and copies its bytes to
`OUT_DIR/generated_cli.rs`. The generated module wrapper refers to that staged
file. Bootstrap and `kit build` therefore use the same source location within
a given Cargo build directory even when their retained outputs have different
paths. The original generated files remain available for direct comparison.

This removes the caller's output directory from the module's source identity.
The Cargo output directory still belongs to the checkout, so independent
builds also need consistent source-path mappings and native toolchain settings.
PR #10 introduced this staging change after the differing PR #8 Windows
executables were built. The owner's completed old-pair image report does not
establish this change's native effect or the mismatch's cause. Current-revision
Windows output still needs its own recorded comparison.

The feature boundary lets the mechanical generator build before CLI Rust
exists. Both source bootstrap and `kit build` reuse `compiler/target` when
the selected target, profile and flags permit it. A first native core build
can take substantial time and memory. Keep the cache when investigating a
later CLI or linker failure.

Progress messages distinguish the public core build, CLI compilation and
native link. Compiler self-regeneration runs only when requested explicitly.
Changing build flags can invalidate a Cargo cache without implying that
self-regeneration is taking place.

The native Kit package sets `strip = "symbols"` in its release profile. This
removes symbol tables from that executable without changing the canonical
compiler crate's optimization setting. Check the resulting size on each
platform before assembling release payload bytes. A Linux size result does not
establish the Windows executable size.

## Native program builds

`build --root`, `build --source` and `build --request` compile through the same
canonical core, then use Topaz policy to write and link a native program.
The requested output directory is created automatically and must be empty or
owned by an earlier native program build. A Kit-build output has a separate
marker and is not interchangeable with a program-build output.

Project manifests and saved project envelopes carry separate filesystem read
and write grants. The generated native host applies those relative to the
application's working directory at launch. Empty or omitted grants provide no
filesystem access for that operation. Standalone source mode and raw Stage 1
requests have no project configuration and retain ordinary process filesystem
access. See [PROJECTS.md](PROJECTS.md) for the full input and grant contract.

The build reports the complete executable path and leaves execution to the
caller. Its native entry uses the public runtime stack contract and preserves
the generated program's exit code or error.

The generated application workspace sets `codegen-units = 1` for release
builds, bounding code-generation parallelism. This is separate from the Kit
and canonical compiler workspace profiles. The validation record includes the
fresh application archive failures that prompted the setting and the clean
builds that passed with it. It does not claim an established root cause or a
guarantee against every native toolchain failure.

## Input maintenance

`compiler/generated/native/compiler.rs` and its manifest identify the
canonical core. The original emission manifest also preserves its original
runtime environment. That provenance is separate from the reduced Kit's
current build inventory.

`compiler/seed/initial/request.json` is the canonical compiler regeneration
input. `core-kit/cli/initial/request.json` is the separate CLI compile input.
The CLI descriptor `core-kit/cli/REQUEST.json` lists exact source and standard
module mappings. The additional `std.toml` boundary declaration uses the
public runtime's TOML support and does not modify the canonical core.

Both the mechanical bootstrap generator and Topaz `kit build` fill current
mapped file contents. They validate canonical identity, source limits and
mapping consistency. `kit build` admits regular files within the checkout
and rejects mapped symlink components. Module additions or removals
also require matching source and directory facts.

Run `kit refresh --check` to inspect the current CLI snapshot and inventories,
or `kit refresh` to update the managed metadata after a source-set change.
The command type-checks the current CLI, protects canonical input hashes and
retains backups before replacing metadata. It needs an existing Kit with the
maintenance command, but no Cargo build, Python, Node or core regeneration.
See [MAINTENANCE.md](MAINTENANCE.md) for its exact scope and recovery record.

Keep the refreshed initial request, `verify-core.data` and the declared
`payload-inputs.data` list with their source changes before publication. Update the
native-program lockfile with any change to its public runtime dependency
graph. Building a local source edit and verifying a committed source snapshot
are separate operations.

## Recorded-input verification

Before running a Kit executable, use the platform verifier from the repository
root.

```sh
sh core-kit/verify-core.sh
```

In an existing PowerShell script environment, use the semantic verifier.

```powershell
.\core-kit\verify-core.ps1
```

After a bootstrap or Kit build, the Topaz command checks the same recorded
byte inventory.

```sh
./out/core-kit-build/topaz-core-kit kit verify
```

The inventory covers canonical source pairs, original snapshot files,
generated core Rust, manifests, requests, standard modules and CLI inputs.
POSIX checks the recorded bytes through a small strict inventory format.
PowerShell additionally checks parsed source, provenance and mapping
relationships. An inventory is a consistency record and can be changed with
its files. It does not independently authenticate a publisher.

The PowerShell verifier decodes file bytes as strict UTF-8 without BOM-based
encoding conversion. Keep `verify-core.data` in the format written by
`kit refresh`; adding a BOM or saving it as UTF-16 or UTF-32 is rejected.
Repository attributes disable automatic Git newline conversion so the recorded
source bytes survive checkout.

## Optional canonical core regeneration

```sh
./bootstrap --verify-core
```

```powershell
.\bootstrap.cmd --verify-core
```

This performs the ordinary bootstrap, builds the public seed, invokes its
bootstrap exchange on `compiler/seed/initial` and compares the regenerated
compiler Rust with the checked-in canonical file. It is separate from ordinary
use, native program builds and CLI development.

The owner has reported a successful Windows core comparison for the canonical
inputs recorded in [VALIDATION.md](VALIDATION.md). Changes to the CLI, native
adapter or packaging around unchanged compiler inputs do not require repeating
that expensive comparison. A matching core is still only one part of a
complete native payload reproduction.

## Optional prebuilt installation

Use `./install-core-kit` or `.\install-core-kit.cmd` for the archive selected
by `core-kit/downloads.data`. `--archive <file>` supplies that exact pinned
archive offline. Download installation is separate from the source bootstrap
and introduces no release dependency into this build path. The current
catalogue lists no archives, so the source bootstrap is the available path.

The initial OS adapter checks the catalogue, archive and candidate executable
before its first execution. Topaz then validates the full payload and current
input identity, stages a version and activates it under
`out/core-kit-installed/<target>/<archive-sha256>/`. The ordinary launcher
uses only the selected installed version and does not download or build.

POSIX installation needs `curl`, `tar` and `sha256sum` or `shasum` alongside
ordinary shell tools. Windows needs PowerShell 5.1 or later, .NET and `tar.exe`.
The Windows installer and launcher honor the host's script execution policy.
They do not bypass it or change policy settings. Native `bootstrap.cmd`
remains available when the host does not permit the PowerShell helpers.

The current catalogue has only its header because no public release assets
have been approved. It must report an unavailable target before network access.
Use source bootstrap for the current development checkout. Do not substitute
fabricated hashes, a moving release or the website CLI.

## Payload reproduction and publication

Use `kit binary-info --file <executable> --other <executable>` to inspect
complete-byte equality, bounded differing offsets, section hashes and
format-specific metadata without running either file. Exit code zero can
accompany `sameBytes` false and is not reproduction evidence by itself.

Assemble each separately recorded build with the same reviewed recipe and
compare the complete candidates.

```sh
./out/core-kit-bootstrap/topaz-core-kit kit payload-assemble \
  --build-dir out/core-kit-bootstrap \
  --recipe core-kit/distribution/linux-x86_64-development.json \
  --out-dir out/payload-a

./out/core-kit-bootstrap/topaz-core-kit kit payload-compare \
  --payload-dir out/payload-a --other out/payload-b
```

The second candidate must already exist. Assembly checks the current CLI
request, successful compiler response, generated source, native image,
dependencies, sizes and required companions. Comparison includes every file,
both manifests and required modes. Keep the independent build histories.
Two matching directories alone do not establish independent construction.

With an approved recipe and native candidate target, promotion can stage
compared bytes and packaging can prepare a named GitHub Release asset.

```text
topaz-core-kit kit payload-promote --payload-dir <first> --other <second> --out-dir <new-stage>
topaz-core-kit kit payload-pack --payload-dir <first> --other <second> --out-dir <new-release> --repository studiohaze/topaz --tag <release-tag> --source-commit <40-character-commit>
```

Both apply native version, source, project and input-verification checks and
recompare the pair. Promotion copies unchanged bytes into the selected new
staging directory. Packaging writes the canonical archive and a catalogue row.
Neither uploads, changes Git or replaces an installation. Publish the reviewed
archive on the named GitHub Release and review its catalogue update separately.

The release archive uses flat canonical ustar and stored DEFLATE in gzip.
Topaz owns construction and parsing, using the existing public runtime codec.
This deterministic representation does not promise compression savings.
The per-file and complete raw-tar ceilings are 256 MiB. Received archives
are bounded at 512 MiB. Git's former per-file binary policy is no longer a
product admission rule.

The catalogue records the archive, executable and actual repository input
identity. It alone is excluded from the release plumbing's reproducible input
set to avoid a self-hash cycle; installer and launcher sources remain included.
A publisher-declared commit is provenance rather than proof of input equality.
The [payload guide](PAYLOADS.md) specifies the exact record and publish checks.

The supplied Linux and Windows x86-64 development recipes keep `releaseReady`
false. The Windows template uses the imports in the owner's PR #8 report.
The retained Windows executables fit the new file ceiling
but still differ. This transition does not establish Windows reproduction,
a minimum supported OS, approved notices or a public release asset. The owner's
[completed PR #8 report](validation-pr8-native-comparison.json) records equal
DLL lists but different timestamps, PDB GUIDs and all reported section hashes.
Do not repeat that inspection. Continue with current-revision native and
installation checks in [LOCAL-VALIDATION.md](LOCAL-VALIDATION.md).

`kit payload-record` and `kit payload-verify` retain their byte-inventory
scope. They do not inspect native compatibility, build independently or
approve a release. The POSIX `core-kit/repro-build.sh` applies stable source-path
mappings. [REPRODUCIBILITY.md](REPRODUCIBILITY.md) defines the comparison
environment. Actual executions belong in [VALIDATION.md](VALIDATION.md) and
remaining work in [STATUS.md](STATUS.md).
