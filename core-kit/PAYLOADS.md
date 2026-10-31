# Preparing and installing Core Kit releases

Git carries the public source, generated canonical core, seed and CLI inputs,
runtime, recipes and verification tools. Optional executable archives are
named assets on GitHub Releases. Their small checked-in catalogue selects
exact bytes for the native target. `dist/` is not a binary storage location,
and Git LFS is not part of this workflow.

Release and installation policy is written in Topaz. OS adapters perform the
first download and establish the executable's expected bytes before its
first run. The native adapter supplies filesystem operations and process
transport without choosing release inputs or approving a platform.

| Command | Contract |
| --- | --- |
| `kit binary-info` | Inspect native image structures and optionally compare two complete files |
| `kit payload-record` | Record an existing directory's bytes and selected checkout inputs |
| `kit payload-verify` | Check that exact directory and input inventory |
| `kit payload-assemble` | Admit a build and recipe, then create a complete new candidate |
| `kit payload-compare` | Compare complete assemblies, manifests and required modes |
| `kit payload-promote` | Approve a native matching pair and copy it to a new explicit staging directory |
| `kit payload-pack` | Approve a native matching pair and write a deterministic archive plus catalogue row |
| `kit install` | Verify the pinned archive, assembled payload and checkout, then activate a local version |

## Current release evidence

The checked-in `core-kit/downloads.data` contains only its schema header.
There are no approved public asset rows. This is intentional. A published
URL, size or hash must come from the actual reviewed release output and must
not be invented to make the installer appear available.

The supplied platform recipes are
[`linux-x86_64-development.json`](distribution/linux-x86_64-development.json)
and [`windows-x86_64-development.json`](distribution/windows-x86_64-development.json).
Both keep `releaseReady` false. The Linux recipe records the observed x86-64
development host with glibc 2.39. The Windows recipe records the ten DLL imports
in the owner's retained PR #8 pair. Neither establishes a minimum supported OS.
They support development assembly and comparison, while native promotion,
packaging and installation require approved release material.

The source-first transition does not resolve the owner's retained Windows
executable mismatch. The 131,391,488-byte files fit the new release file-size
profile, but equal source output and an admitted size are not native byte
reproduction. The owner completed the PR #8 inspection and supplied
[its JSON report](validation-pr8-native-comparison.json). The DLL lists match,
but timestamps, PDB GUIDs and all reported section hashes differ. Cause remains
unproven. Preserve that evidence and follow the current-revision native and
installation gates in [LOCAL-VALIDATION.md](LOCAL-VALIDATION.md). Do not repeat
the completed old-pair inspection. Executed results belong in
[VALIDATION.md](VALIDATION.md), including the revision and environment tested.

## Inspect the native image

```sh
./out/core-kit-bootstrap/topaz-core-kit kit binary-info \
  --file out/core-kit-bootstrap/topaz-core-kit \
  --other out/core-kit-build/topaz-core-kit
```

The report compares complete bytes and includes `sameBytes`, at most 16
differing offsets, section hashes and supported ELF, PE or Mach-O metadata.
Omit `--other` for one image. Inspection can return success while reporting
different files. It does not execute an image, verify a signature or establish
minimum platform support. Inputs must be nonempty regular files no larger
than 512 MiB. The public filesystem API reads complete buffers, so this is an
admission bound rather than a streaming-memory guarantee.

## Assemble and compare candidates

A completed ordinary bootstrap or `kit build` retains `cli.request.json`,
`cli.response.json`, `generated_cli.rs` and the native executable. Assemble
each independently recorded build into its own new directory.

```sh
./out/core-kit-bootstrap/topaz-core-kit kit payload-assemble \
  --build-dir out/core-kit-bootstrap \
  --recipe core-kit/distribution/linux-x86_64-development.json \
  --out-dir out/payload-a

./out/core-kit-bootstrap/topaz-core-kit kit payload-compare \
  --payload-dir out/payload-a --other out/payload-b
```

The comparison assumes the second candidate has already been assembled.
`--recipe` takes a path under `core-kit/distribution` relative to the
checkout. The CLI accepts either slash separator and records the recipe
with forward slashes in the assembly. Other explicit paths are relative to the caller.
Each output directory must be absent. Admission precedes exclusive creation.
If copying fails, retain that new directory for diagnosis and use a different
destination for another attempt.

Assembly requires the request to describe the current CLI sources and the
successful compiler response to contain exactly the retained generated Rust.
The executable must match the recipe's target and native format. Every
reported loader dependency must fit the recipe's system-library allowance or
an explicitly mapped runtime companion. Unknown dependencies and unsupported
runtime search paths fail.

The version 2 recipe admits files of at most 268,435,456 bytes. This limit is
independent of GitHub's Git blob policy. Each file must be nonempty. Packaging
also bounds the complete uncompressed archive, including headers and padding,
to 256 MiB. A collection of individually admitted files may therefore still
be too large to package.

Thirteen baseline component and notice companions are required. They retain
the inherited terms and inventories, Kit component and notice-scope records,
Rust standard-library notices and provenance, seed-runtime authorization,
source-component inventory and upstream port record. Additional runtime files
come from reviewed mappings under `core-kit/distribution/runtime/<target>`.
Assembly does not collect arbitrary files from the build host.

The output contains those companions, the executable and `generated_cli.rs`.
`assembly.json` records the recipe, input identities, native profile,
dependencies, platform requirements, file roles and modes. `payload.json`
records exact files and repository inputs. Both are verified before success.

Unix executables require `0755`; companions and PE files use `0644`. Windows
can clear the read-only attribute but does not report Unix permission bits.
It cannot validate a Unix executable mode. The mode contract does not inspect
ACLs or ownership.

Complete comparison requires two distinct canonical directories. It checks
all file bytes, both manifests and required modes against the current checkout.
Extra, missing, changed or reordered files fail. Comparison does not execute
the candidates. Equal directories do not reveal whether their build histories
were independent. Keep the source revision, cache and source locations,
compiler, linker/SDK, flags, environment and platform test results separately.

## Approve and stage native bytes

```sh
./out/core-kit-bootstrap/topaz-core-kit kit payload-promote \
  --payload-dir out/payload-a --other out/payload-b \
  --out-dir out/release-stage
```

This requires `releaseReady` in the checked-in recipe and a running Kit for
the candidate's native target. Both supplied development recipes deliberately
fail this approval gate.

Approval compares both candidates and runs the selected executable's
version, source, project and input checks. Its `kit build-id` must match
the SHA-256 of the generated CLI Rust in the assembly. A failed relink in
a reused output directory can leave an older executable next to newer
generated Rust. Use a fresh output directory if these identities differ.
Approval compares the candidates again after execution. Promotion copies them to the new explicit
output directory, retains the original manifest and modes, copies the
executable last and rechecks the destination. It refuses an existing output.

Promotion does not build, install, upload, replace a release or modify Git.
Its staging directory is suitable for review or other local handling. Release
packaging can approve the two candidates directly and does not require this
extra staging copy.

## Pack a named release

```sh
./out/core-kit-bootstrap/topaz-core-kit kit payload-pack \
  --payload-dir out/payload-a --other out/payload-b \
  --out-dir out/release-package \
  --repository studiohaze/topaz \
  --tag core-kit-<release-version> \
  --source-commit <40-character-source-commit>
```

Use the actual repository, a specific release tag and the actual source
commit declaration. The output directory must be absent. The command applies
the complete native approval gate, writes
`topaz-core-kit-<target>.tar.gz`, a one-row `downloads.data` and a
`package.json` receipt, and checks the result. It performs no network operation or Git mutation.

The publisher must retain evidence that the compared candidates came from
independent builds. The packaging command cannot infer that history. The
source commit is supplied provenance and is distinct from the actual source
input identity computed by the command.

### Archive representation

Archive construction and parsing live in Topaz. The archive is canonical flat
ustar wrapped in the public runtime's gzip stored-DEFLATE representation.
Stored blocks deliberately avoid a new codec dependency and do not promise
compression savings. Archive bytes are reproducible for the same payload
bytes and modes.

Entries are ordered by their portable flat filenames. Headers, file modes,
uid, gid and zero timestamps have one accepted representation. Symlinks,
directories, traversal, prefixes, long-name extensions, duplicate or aliased
names and arbitrary archive metadata are rejected. The reader checks lengths,
header fields, padding and footer bytes before materializing any payload file.

The complete raw tar is limited to 268,435,456 bytes. An individual file shares
that ceiling. Received gzip archives are limited to 536,870,912 bytes. Bounds
are checked even when a catalogue provides a length. Complete-buffer decoding
can require substantial memory and is not advertised as streaming extraction.
OS tar is used only by the initial adapter to write the fixed executable
member to a private file after the complete archive hash has matched. Topaz
performs full archive validation.

### Download catalogue

`core-kit/downloads.data` is strict ASCII with LF line endings and a final LF.
The first line is the schema identifier.

```text
topaz.core-kit-downloads/v1
```

Zero through six rows follow, sorted by target with no duplicate target. Each
row has exactly these ten pipe-separated fields.

```text
TARGET|REPOSITORY|TAG|SOURCE_COMMIT|INPUT_SET_SHA256|ASSET_NAME|ASSET_BYTES|ASSET_SHA256|EXE_BYTES|EXE_SHA256
```

The repository is a portable `owner/name`. The tag is a specific admitted tag,
not `latest`. The source commit is lowercase 40-character hexadecimal. Digests
are lowercase 64-character hexadecimal. Byte counts are canonical positive
decimal integers. The asset name is fixed by the target as
`topaz-core-kit-<target>.tar.gz`. Native target names are listed in
[the distribution guide](../dist/README.md).

`INPUT_SET_SHA256` is SHA-256 of `json.stringify(repositoryFiles(root))` using
the canonical sorted file records, with no appended LF. The records contain
paths, byte counts and SHA-256 values. The source catalogue is excluded from
this inventory, while the installer and launcher sources remain included.
This prevents the catalogue's archive digest from entering the archive's own
source digest and producing a cycle.

A later commit that changes only the catalogue can select an archive built
from the same actual input set. A commit label alone does not prove those
inputs match. Installation recomputes the actual identity and requires exact
equality. Catalogue parsing and input inventory maintenance both preserve the
exclusion rather than silently admitting a self-referential list.

## Publish the reviewed output

1. Record the independent builds and native target results. Review component
   notices and the release recipe's platform claim.
2. Package the approved pair with the final repository, tag and source commit.
3. Upload the exact archive bytes as the named asset on that GitHub Release.
   Do not repack it with another tar or gzip tool.
4. Review the generated row, confirm the uploaded asset's byte count and
   SHA-256, and add the row to the source checkout's catalogue in target order.
5. Exercise first-run installation from the final checkout and asset on its
   native target, then retain that result with the release evidence.

The installer constructs the named HTTPS GitHub asset URL.

```text
https://github.com/REPOSITORY/releases/download/TAG/ASSET_NAME
```

It has no
arbitrary URL, repository or moving-version override. Publication is separate
from the local packaging command. No executable or archive needs to enter Git
history, and source bootstrap continues to need no release asset.

## First-run installation

```sh
./install-core-kit
./install-core-kit --archive /path/to/the-exact-pinned-archive.tar.gz
```

```powershell
.\install-core-kit.cmd
.\install-core-kit.cmd --archive C:\Downloads\the-exact-pinned-archive.tar.gz
```

The OS adapter parses the complete checked-in catalogue and selects the
native target. A missing target fails before any network request. Downloads
use HTTPS to the named GitHub Release. Offline mode supplies the same pinned
bytes without a network request. The caller cannot override the expected
length or digest.

Before the first candidate execution, the adapter checks the archive's length
and SHA-256, extracts only the fixed executable member to a private file and
checks that executable's length and SHA-256. It does not invoke an unchecked
candidate to calculate its own initial expected identity or unpack arbitrary
archive paths onto disk.

It then invokes `kit install --archive <absolute-file>` with
`TOPAZ_CORE_KIT_ROOT` selecting this checkout. Topaz rechecks the catalogue,
native target, recorded inputs, current input identity, archive and executable
bytes. It decodes the full canonical archive before creating payload files.
The assembled payload must match the current release recipe and input set.
Native candidate checks must pass before activation.

The initial executable must start using the release's declared system
dependencies. `kit payload-pack` inspects its actual imports and refuses an
import satisfied only by a bundled runtime mapping. The same rule applies
when reconstructing an archive for explicit installed-payload checking.
Assembly and explicit promotion retain support for runtime companions, and
unreferenced companions remain eligible for a download. The OS adapters keep
their fixed executable-only extraction path.

Test that first run on the declared minimum platform without development
tools. A successful run on a build machine can inherit runtime libraries that
an ordinary user lacks. In particular, the retained Windows images import
`vcruntime140.dll`. The final Windows release must record and test the actual
runtime prerequisite, or use a separately validated build that removes that
dependency. Merely listing a DLL as a system dependency does not establish
that it exists on the supported host.

Windows helpers use `powershell.exe` with `-NoProfile`, `-NonInteractive` and
`-File`, and honor the host execution policy. They require Windows PowerShell 5.1 or
later, its .NET runtime and `tar.exe`. The installer does not bypass policy or
change machine settings. `bootstrap.cmd` remains the source route if scripts
are unavailable. POSIX installation requires `curl`, `tar` and `sha256sum` or
`shasum`, plus ordinary shell utilities. Neither route requires Python, Node,
jq or the GitHub CLI.

## Installation state and failures

The version path is
`out/core-kit-installed/<target>/<archive-sha256>/`. It contains only a
`payload/` directory and `installation.data`. The receipt is one exact
LF-terminated line.

```text
topaz.core-kit-installation/v1|TARGET|ASSET_SHA256|INPUT_SET_SHA256
```

Topaz owns an exclusive target installation lock, writes a separate stage,
validates it and renames the complete new version directory into place.
Existing versions remain untouched. Reinstalling the selected version verifies
its files and receipt and returns without replacing it. Installed-version
verification also reconstructs the complete canonical archive from the local
payload and requires its length and SHA-256 to match the catalogue. Updating
a local `payload.json` to describe changed files cannot satisfy that pinned
archive check. The command refuses
symlinked cache components and unexpected existing state.

If the selected version's receipt or executable is damaged, keep the reported
version directory for inspection. Move that directory to an unused sibling
name after any active installation has finished, then run the installer with
the matching archive again. Older versions and their receipts stay in place.
An input-set error calls for the matching source checkout and catalogue.

A failed stage remains available for diagnosis and is never selected as a
completed version. Keep its path and diagnostic. If an interruption leaves a
lock, first establish that its process has ended and inspect the stage before
removing a stale lock. An ordinary failed admission does not justify deleting
the installed cache or overwriting old evidence.

The root launchers choose only the current catalogue's exact completed
installation using the catalogue and receipt. They make no network request,
build, update or full source check on every invocation. An installed Kit can
therefore refresh and build a modified source tree. Use `kit install --check`
for explicit full installation and current-input verification, including the
canonical archive reconstruction. This work is part of installation and
explicit checking, not every ordinary command. `bootstrap` still
writes `out/core-kit-bootstrap`; run its executable directly. It does not
activate a catalogue version. Keep the checkout with an installation because
commands depend on its sources, standard modules and build templates.

## Byte inventories and evidence boundaries

`kit payload-record --target <target> --payload-dir <directory>` records an
already prepared flat directory. `kit payload-verify --payload-dir <directory>`
checks its exact files and the selected repository inputs. These commands
retain their `recorded-inputs-only` scope. They do not assemble, execute or
independently rebuild the candidate. Recording refuses an existing manifest.

`payload.json` contains sorted records for every payload file except itself
and all paths in `payload-inputs.data`, including that list's own bytes. The
list contains paths rather than its own hash, avoiding an input-list cycle.
The selected input set includes the canonical source and seed copies,
generated core, requests, public Cargo workspace, CLI and native source,
initial mappings, build templates, launchers, installers, bootstrap and
verification tools, smoke fixtures, recipes and component notices.

The list is curated. [Maintenance](MAINTENANCE.md) updates its managed trees;
new dependencies elsewhere need explicit review. It does not inventory every
Git file. Documentation status reports, caches, local installations, build
outputs and the release catalogue are outside that reproducible input set.

Verification collects required files independently of manifest declarations
and rejects missing, extra, reordered or changed records. Canonical JSON,
including its final LF, is required. Unknown or duplicate fields, unsafe
paths, symlinks, case aliases and reserved Windows names fail. Inventory bounds
remain 256 payload files, 1,024 repository files and a 1 MiB manifest/list.
Its general per-file read ceiling is 512 MiB; release assembly and archive
packaging impose the smaller limits described above. Keep inputs stable
because these complete-buffer operations are not filesystem snapshots.

An inventory proves observed byte consistency. Archive equality proves equal
encoding of its inputs. Native approval adds its stated execution checks.
None independently authenticates the publisher, establishes independent build
history or grants a source license.

The inherited third-party notices describe the website's larger product.
`COMPONENTS.json` and `NOTICE-SCOPE.md` describe the reduced Kit's selected
components and those inherited notice boundaries. Seed runtime authorization
is recorded in `compiler/seed/runtime.json`; [UPSTREAM-PORT.md](UPSTREAM-PORT.md)
records the selected public port. Platform and system components still need
their applicable terms and review. Keep release claims within the actual
native evidence in [VALIDATION.md](VALIDATION.md) and [STATUS.md](STATUS.md).
