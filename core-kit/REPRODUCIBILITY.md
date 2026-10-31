# Comparing complete Core Kit executables

Core regeneration and executable reproduction answer different questions.
`bootstrap --verify-core` compares regenerated compiler Rust with the canonical
compiler Rust. This procedure compares the complete native Core Kit executable
produced from the same source inputs in different build locations, then
compares the assembled payload and release archive that would be published.

## POSIX path-normalized build

Use the already-installed Rust/Cargo 1.96.0 and native linker.

```sh
./core-kit/repro-build.sh
# A custom output may be outside the checkout and may contain spaces.
./core-kit/repro-build.sh ../kit-output
```

The default output is `out/core-kit-repro/topaz-core-kit`. This small wrapper
uses the ordinary source bootstrap and does not regenerate the canonical core.
It applies the following Rust flags to the target crates, including their
dependencies.

```text
--remap-path-prefix=<physical checkout root>=/topaz-core-kit
--remap-path-prefix=<physical output>/generated_cli.rs=/topaz-core-kit/generated/cli.rs
```

The generated-file mapping comes last and retains the explicit mapping for
the selected output. `CARGO_ENCODED_RUSTFLAGS` preserves spaces and Unicode
within each compiler argument. Paths containing its U+001F separator are
rejected. An existing `RUSTFLAGS` or `CARGO_ENCODED_RUSTFLAGS` is rejected
rather than silently replaced. A generated-file symlink is also rejected.

The current native build script copies the selected generated Rust bytes into
`OUT_DIR/generated_cli.rs` before compiling the module. The caller's bootstrap
or `kit build` output directory no longer becomes that module's compiled source
path. This staging location remains inside Cargo's build directory and is
covered by the checkout mapping for the standard local target directory.
An externally configured Cargo target directory needs its own recorded recipe.

Earlier Linux comparisons used the path mappings to remove visible generated
CLI and adapter source paths. The fixed staging location is an additional
change, not a new Windows reproduction result. None of these changes modifies
the canonical compiler source or its generated Rust. A changed flags
fingerprint can still trigger a substantial Cargo rebuild even though compiler
self-regeneration remains disabled.

The wrapper normalizes source paths. It does not select a linker/SDK, erase
inherited Cargo configuration, or certify an arbitrary build environment.
Record those inputs and compare the actual output before claiming a result.

## Windows path-normalized build

`core-kit/repro-build.ps1` applies the same two mappings on Windows and then
runs the ordinary `bootstrap.cmd`. It needs Windows PowerShell 5.1 or later
where local scripts are already allowed, plus the installed Rust/Cargo 1.96.0
and MSVC linker.

```powershell
.\core-kit\repro-build.ps1
# A custom output may be outside the checkout and may contain spaces.
.\core-kit\repro-build.ps1 ..\kit-output
```

The default output is `out\core-kit-repro\topaz-core-kit.exe`. The checkout
maps to `/topaz-core-kit` and the selected output's `generated_cli.rs` maps to
`/topaz-core-kit/generated/cli.rs`. The script starts `bootstrap.cmd` through
the same full checkout path that it maps, so every absolute source path Cargo
passes to rustc begins with that prefix. Separators after the mapped prefix
stay as Windows wrote them. An existing `RUSTFLAGS` or
`CARGO_ENCODED_RUSTFLAGS` is rejected, as is a generated file that is a link.

The final link already uses `/INCREMENTAL:NO`, `/Brepro` and `/DEBUG:NONE`
from the native build script. The wrapper does not select the Visual Studio
or Windows SDK version. Record them with the result.

## Cargo package identity

The native manifest is deliberately under `compiler/core-kit-native/`, within
the `compiler` workspace. It points to the Rust sources under `core-kit/native`;
no adapter implementation has to be duplicated or moved.

Cargo 1.96 hashes a path package relative to the workspace only when the package
is inside that workspace. For an external member, its absolute source URL
enters the crate metadata before rustc applies source-path mappings. This was
the remaining cause of different native binaries after the visible source
paths had already been normalized. Moving only this manifest keeps the public
core and runtime package identities unchanged while fixing the adapter's
identity. The ordinary `cargo -p topaz-core-kit-native` commands are unchanged.

The relevant implementation is pinned to the Cargo used for validation in
[source identity](https://github.com/rust-lang/cargo/blob/30a34c6821b57de0aaec83a901aca39f88f6778c/src/cargo/core/source_id.rs#L564)
and [compilation metadata](https://github.com/rust-lang/cargo/blob/30a34c6821b57de0aaec83a901aca39f88f6778c/src/cargo/core/compiler/build_runner/compilation_files.rs#L719).

## Independent source builds

Use the same reviewed commit in two fresh worktrees. They share Git objects,
but each has its own source directory and initially empty Cargo target directory.
Do not copy `compiler/target` from one build into the other. The example below
expects the four sibling source/cache paths not to exist yet.

```sh
(
set -eu
revision=$(git rev-parse HEAD)
git worktree add --detach ../kit-source-a "$revision"
git worktree add --detach ../kit-source-b "$revision"

(
  cd ../kit-source-a
  mkdir ../kit-cargo-a
  export CARGO_HOME="$(CDPATH= cd ../kit-cargo-a && pwd -P)"
  ./core-kit/repro-build.sh
)
(
  cd ../kit-source-b
  mkdir ../kit-cargo-b
  export CARGO_HOME="$(CDPATH= cd ../kit-cargo-b && pwd -P)"
  ./core-kit/repro-build.sh
)

cmp ../kit-source-a/out/core-kit-repro/generated_cli.rs \
    ../kit-source-b/out/core-kit-repro/generated_cli.rs
cmp ../kit-source-a/out/core-kit-repro/topaz-core-kit \
    ../kit-source-b/out/core-kit-repro/topaz-core-kit
)
```

Stop on a failed build before comparing outputs. A fresh `CARGO_HOME` separates
the caches but does not remove ancestor `.cargo/config` files, environment
overrides or compiler wrappers. Record any such inputs. No extra Topaz download,
Python, Node, jq or package-registry dependency is introduced by these commands.

Run the existing smoke against each resulting executable and preserve the
results. When an output is outside its data checkout, select that checkout with
`TOPAZ_CORE_KIT_ROOT`; the smoke scripts do this explicitly. A binary kept
under its own checkout can also discover the data from its executable location.

## Retained Windows mismatch

The owner reported Windows results at PR #8 merge
`b6a97245189365f93801f9e996c3c3152bcd3e3e`. Ordinary bootstrap, native smoke and
the actual Topaz `kit build` succeeded. The two 690,692-byte generated CLI Rust
files matched through `fc /b` and had SHA-256
`807d2c17195263f90a318c77a997d9b314f23739d4237576b3b3c30137dc524a`.

The two executables were both 131,391,488 bytes but did not match.

| Output | SHA-256 |
| --- | --- |
| Bootstrap | `ea2b4795c6ebdd9e5dffa29e30a21b8e7a44666c5970395657e96baea5238054` |
| `kit build` | `0e2d3d05a2498f3fb4eeeb3cc421977f58d370d2cca413c15810ffeadeadaad3` |

The owner subsequently completed the bounded comparison with `kit binary-info`
and supplied [the original JSON report](validation-pr8-native-comparison.json).
Its file identities match this recorded PR #8 pair. Both images have the same
imported DLLs and section boundaries. PE timestamps are `1793396347` and
`1793396526`; the PDB GUIDs differ. Every reported section digest differs,
including `.text`, `.rdata`, `.data`, `.pdata` and `.reloc`.

The observed mismatch is therefore not confined to timestamp and PDB identity
metadata. The report does not identify the responsible build decision or
establish a root cause. These remain supplied local observations, not an
independent reconstruction of the native build environment.

Preserve the retained PR #8 worktree, its Cargo cache, both outputs,
the smoke logs under `out\core-kit smoke-17111-2065` and the supplied report.
The old-pair inspection is complete. Do not repeat it, overwrite those outputs,
remove their cache or rerun `--verify-core` as part of this transition.

### Matching Windows EXEs and payloads at PR #13

PR #10 introduced fixed generated-source staging. At PR #11 the generated
Rust and PE code sections matched, while the complete EXEs still differed.
With the PR #13 release linker settings, bootstrap and `kit build` produced
identical full EXEs on Windows x64. The two payloads also compared equal.
Both links ran in one checkout with the same Cargo cache.
[PR #13 native Windows result](VALIDATION.md#pr-13-native-windows-result)

## Compare the complete candidate

Use `kit payload-assemble --build-dir <build> --recipe <repository-file>
--out-dir <new-directory>` separately for each build. The assembly records the
current CLI input, generated source, image dependencies, recipe, required
notices and file modes. It also writes the existing byte inventory.

`kit payload-compare --payload-dir <first> --other <second>` validates each
candidate against the selected checkout and compares complete file contents,
including both manifests, plus the required modes. It refuses to compare one
canonical directory with itself. Two distinct directories still do not prove
independent construction. Preserve the build histories described above.

The supplied Linux and Windows x86-64 development recipes keep their
`releaseReady` fields false. They allow assembly and comparison but refuse
native release promotion and packaging. The download catalogue currently
contains only its header. No approved public release asset is represented.

A reviewed release recipe permits native candidate version, source check,
project check and `kit verify` gates. The candidate must also report the digest
of the generated Rust included in its assembly. Approval compares the candidates again
after execution. `kit payload-promote --payload-dir <first> --other <second>
--out-dir <new-stage>` copies unchanged files to an explicit staging location
and refuses an existing destination. It does not install or modify Git.

## Compare the archive and source identity

`kit payload-pack` approves the native pair, encodes the complete payload and
writes its named archive, catalogue row and package receipt. Repeat packaging
into a different new output directory with the same two candidates, repository,
tag and source commit declaration. Compare both archives directly, along with
the generated records. That comparison checks the packaging representation.
It does not add independence to the builds from which those candidates came.

The Topaz archive writer fixes flat ustar ordering, headers, modes, uid, gid,
timestamps and padding. It wraps that tar using the public runtime's canonical
gzip stored-DEFLATE output. Equal admitted inputs produce the same archive
bytes without a platform-specific external compressor. Stored blocks are not
a compression-ratio optimization. Individual files and the whole raw tar are
bounded at 256 MiB, and received archives at 512 MiB.

The catalogue records both the archive and executable SHA-256 values. Its
input-set digest is SHA-256 of the canonical sorted repository file-record
JSON, without an appended LF. Installer sources are included in these records.
The catalogue itself is excluded to avoid a cycle in which the archive's hash
changes its own recorded source inputs.

The publisher's declared source commit is distinct from that actual input
digest. A later catalogue-only source commit can select the same reproducible
input set. Installation recomputes the current records and requires their
identity to match. A declared commit does not independently establish a build
or substitute for the byte comparison.

Preserve the archive bytes produced by packaging. Recompressing them with a
different tool changes the transport identity even if extracted files match.
Publish those same bytes to the exact named GitHub Release asset and check its
length and digest before reviewing the catalogue update. First installation
checks archive and executable identities before running the candidate, then
Topaz validates the complete payload and current source inputs. Installed
verification also re-encodes the local payload into its canonical archive and
compares its length and hash with the pinned release, binding all companions
and manifests to the selected bytes. This happens during installation or
explicit `kit install --check`. Ordinary launchers use the installed version
without that reconstruction, downloading or rebuilding.

[PAYLOADS.md](PAYLOADS.md) specifies archive admission, notice scope and the
publication procedure. The source bootstrap needs neither the catalogue nor
a release download.

## What a matching pair establishes

Matching complete executable bytes establish reproduction for the recorded
source, native build recipe and environment. The comparison does not establish
a compiler trust proof, reproducibility on another OS/toolchain, or a minimum
supported OS/ABI. Cargo caches, build-script executables and diagnostic logs
are not the shipping payload and are not required to match this comparison.

The Linux development results and their core optimization override are recorded
in [VALIDATION.md](VALIDATION.md). Independent release builds in separate clean
locations and caches remain to be compared on Windows and the other intended
platforms. Rust's source mapping alone does not normalize every linker-generated
path or platform timestamp.

Before publication, establish the recipe's platform requirements and required
component notices, assemble the intended payload and preserve the independent
comparison evidence. Publish the same archive bytes that passed packaging
and review the catalogue update separately. Equality
does not authenticate the publisher or create a blanket license for the
included sources. This procedure does not itself release a prebuilt Kit.

Additional references are [Rust source-path mapping](https://doc.rust-lang.org/rustc/remap-source-paths.html)
and [Cargo compiler flags/configuration](https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags).
