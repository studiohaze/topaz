# Core Kit validation record

## CLI, installation recovery and native adapter

Linux x86-64 validation used pinned Rust/Cargo 1.96.0 and the public
compiler. The runner had 8 GiB of memory. After two native code-generation
attempts hit that limit, rustc emitted LLVM bitcode for the unchanged seed
compiler with `opt-level=0`. The bundled LLD 22.1.2 lowered its 72 code
generation units separately. All archive members and the Rust metadata were
checked before linking. Repository build profiles were unchanged. As a
control, the resulting compiler reproduced the previously recorded PR #12
request, response and 2,102,489 bytes of generated CLI Rust exactly.

- Twenty library tests and one bootstrap generator test passed with Cargo.
  The tests cover interrupted reads, stream limits, process arguments,
  exact input paths and preserving an existing file on a failed copy.
- The repository smoke with `--native-build` passed, including exact output
  from the standalone sample and saved project, the linked-source digest,
  source refresh checking and payload verification. Both source inventory
  verifiers passed.
- The actual Kit passed fourteen PE inspection and comparison cases.
  These included LLVM-linked images, import descriptors with and without
  a lookup table, invalid DLL names, an unterminated import table, CodeView
  bounds and the first differing offsets between two images.
- Fifteen POSIX launcher/installer cases and fifteen PowerShell cases passed.
  The PowerShell checks used 7.6.9 on Linux, OS shims and a small first-stage
  executable. They covered archive extraction, working directory, arguments,
  cleanup, exit status and refusal before executing a candidate with a bad hash.
- In an isolated OS temporary checkout, 23 product commands returned the
  expected results (12 successes and 11 refusals). Both recipe separators
  produced equal candidates. A stored recipe with backslashes was rejected.
  The POSIX installer installed the private archive offline. Removing the
  receipt produced the damaged version's exact path. Moving just that version
  aside allowed reinstallation from the original archive and installed-byte
  verification. A changed checkout and a wrong archive produced distinct errors.

The tested Linux executable was 79,365,024 bytes with SHA-256
`aeb25e2769c311b4283c4699e5088c6f7ea340ce7d72cdab0c2ebd5203d1ba34`.
Its linked CLI Rust was 2,114,898 bytes with SHA-256
`654a196520bfee7ea62788e6cc40ac283abdef6fca0c69f73b007d7dc5564b64`.
Official bootstrap and Topaz `kit build` produced the same generated Rust and
complete executable in one checkout with a shared cache. Candidates assembled
separately from those two outputs passed `payload-compare` across 16
inventoried files, 315 repository inputs and the required file modes.
The private recovery fixture archive was 82,214,037 bytes with SHA-256
`0377abeeecd7aa51e5f8aefee9f158ca65c3b1bfc46319f57f618bc766ed805b`.
An earlier workspace run found an incomplete version directory unexpectedly
present on retry. The isolated rerun and a separately observed recovery from
the first fixture both passed. The cause of the first observation is unknown.

A pre-guard Kit produced and installed a private archive of 82,185,872 bytes
with SHA-256 `5ad96ee530c2033daee4795870cf270477900cf1f8fe8ec6f530985e1239cea3`.
The old executable had no `kit build-id` command. The updated Kit, pointed
at that installation's original source and catalogue, passed `kit install --check`.

### Failed relink in a reused output directory

The control began with a successful `kit build`. Changing the help text kept
the displayed Kit version constant. The actual compiler wrote a new request,
response and generated Rust before an injected Cargo failure returned exit 37
at the final native link. The old executable remained and printed its old help.
Two candidates assembled separately from that single failed output passed
comparison. Before the build-ID check, `payload-pack` actually accepted them
and wrote a private archive with SHA-256
`2c6c6285a9abbfbfb1fb42b118e45fc44be25ce70041e3e60fdc45053f873f2f`.

The new packer rejected those same candidates because their old executable
could not report its build ID. A second, controlled case paired the
failed build's compiler files with a genuine updated executable. The EXE
reported `654a196520bfee7ea62788e6cc40ac283abdef6fca0c69f73b007d7dc5564b64`;
the candidate's generated Rust measured
`4c02492e51dffbbd417fe2566c1259e52d355d471778d5c4a32aca2c537f9209`.
Both assemblies passed structural comparison. Native promotion and packaging
each rejected the mismatch before creating their output directory. The
original failed build and its control archive retained their hashes. This
check identifies the linked CLI Rust. Platform toolchains and the rest of the
native binary still require complete payload comparison and build records.

With Rust/Cargo 1.96.0 and LLD 22.1.2, two Windows x64 fixture links from
identical source bytes in distinct paths produced the same PE. Changing
only a source comment changed the embedded digest. The owner subsequently
built and installed the full Kit on Windows 11 x64.
[PR #15 native Windows result](#pr-15-windows-build-and-offline-installation)

## PR #14 Windows offline installation

The owner ran the handoff from merge `77024fd` on Windows 11 x64 with
Windows PowerShell 5.1, tar 3.8.9 and CurrentUser RemoteSigned. Both
retained PR #13 executables matched their recorded hash before the run.
In a detached worktree, the owner enabled the test recipe, refreshed and
verified the inputs, assembled one candidate from each build, compared the
pair and packed the archive. [PR #14](https://github.com/clavef/topaz_note/pull/14)

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| Windows x64 Kit EXE | 145,224,192 | `2fd7f98d5faf00d13c331703bd37b0603216b15503e4113607a7946f9ee169c9` |
| Offline test archive | 148,065,846 | `1c1f745b9388b685ddd3b3bd4ffa726281d8d1d43af0ebe1e9c647f3cf85743a` |

`install-core-kit.cmd --archive` installed those bytes offline. The installed
launcher passed `--version`, `check --root examples/core-kit/project` and
`kit install --check`. The fixture and matching catalogue remain under
`D:\Projects\topaz-offline-a8c14a618d0f4c49a961bf4766ad0b58`.
The public catalogue has no release row. First installation on a clean host
and reproduction with independent caches remain open.
[Clean Windows handoff](LOCAL-VALIDATION.md#first-run-on-an-ordinary-windows-host)

## PR #16 no-build-tool installation fixture

The retained PR #15 archive was copied to a separate source fixture that
excluded `.git`, `out` and `compiler/target`. The installer process exposed
PowerShell 5.1, `tar.exe` and Windows system paths, but no Rust, Cargo or
Visual Studio paths. `install-core-kit.cmd --archive` performed the first
offline installation, and the installed launcher passed `--version`, the
example project check and `kit install --check`.

The archive was 148,166,203 bytes with SHA-256
`cc745fab9e18c678def212cc46691f1fe1b15f94f76ba5f6d924557eaa5e0108`.
The receipt and host details are retained in
[validation-pr16-no-build-tools-install.json](validation-pr16-no-build-tools-install.json).
The fixture is `D:\Projects\topaz-clean-host-pr16-20261031`.

This is no-build-tool evidence on the existing Windows 11 host, not an
independent clean-machine or separate-kernel result. A Windows Sandbox, VM or
other Windows x64 host is still required for that release gate.

## PR #15 Windows build and offline installation

The owner tested merge `2d3af629` on Windows 11 x64 with Rust/Cargo 1.96.0,
Windows PowerShell 5.1, tar 3.8.9 and `CurrentUser RemoteSigned`. Native smoke
passed with the bootstrap Kit. The rebuilt Kit passed version, project and
input checks. Running each EXE with `kit build-id` returned the generated
Rust digest below.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| Windows x64 Kit EXE | 145,312,256 | `5e5fd737f6d8cdd576c02113fb9fe9c0c82e7f3762b524af1f860a4ec52bcc10` |
| Generated CLI Rust | 2,114,898 | `654a196520bfee7ea62788e6cc40ac283abdef6fca0c69f73b007d7dc5564b64` |
| Offline test archive | 148,166,203 | `cc745fab9e18c678def212cc46691f1fe1b15f94f76ba5f6d924557eaa5e0108` |

The two complete PE files were byte-identical. Their separate payload
assemblies matched across 16 files, 315 repository inputs and the required
modes. The bounded comparison is retained in
[validation-pr15-native-comparison.json](validation-pr15-native-comparison.json),
and the package receipt is retained in
[validation-pr15-offline-package.json](validation-pr15-offline-package.json).

In a separate build copy, the owner changed one byte of `generated_cli.rs`
and kept the successful compiler response intact. `payload-assemble` compares
those two source texts before opening the EXE. It rejected the edited file
and left the destination absent.

The successful Windows `payload-pack` exercised the matching path of the
native build-ID check. At release approval the Kit executes the candidate and
compares its reported ID against the assembly's generated Rust digest. The
Windows run did not exercise rejection at this later check. The earlier Linux
test reached it and observed both promotion and packaging refuse the mismatch.
[Failed-relink test](#failed-relink-in-a-reused-output-directory) ·
[Windows check using the retained builds](LOCAL-VALIDATION.md#windows-negative-control-for-the-linked-cli)

The private archive was packed in a detached fixture and installed with
`install-core-kit.cmd --archive` without network access. The installed
launcher passed `--version`, `check --root examples/core-kit/project` and
`kit install --check`. The fixture remains outside the repository at
`D:\Projects\topaz-offline-pr15-fe2bfbfb8d794bbeb6292f702fdbdaa2`.
The development recipe remains `releaseReady: false`, so this does not claim
public release approval, independent-cache reproduction or clean-host
installation.

## PR #13 native Windows result

On 31 October 2026 the owner tested the PR #13 source on Windows x64 with
Rust/Cargo 1.96.0. The PR head `7b716ca` and its merge `cf4df7c` have the
same Git tree. The run used one worktree and shared its Cargo target directory
between bootstrap and `kit build`. [PR #13](https://github.com/clavef/topaz_note/pull/13)

| Check | Reported result |
| --- | --- |
| Bootstrap and native smoke | Passed with the new bootstrap EXE supplied as the first argument to `smoke.cmd`. |
| Topaz `kit build` | Passed. The rebuilt Kit ran `--version` and `kit verify`. |
| Generated CLI Rust | The two `generated_cli.rs` files matched with `fc.exe /b`. |
| Complete EXE | Both were 145,224,192 bytes with SHA-256 `2fd7f98d5faf00d13c331703bd37b0603216b15503e4113607a7946f9ee169c9`. The binary comparison found identical bytes, PE section hashes and imported DLLs. |
| Development payload | Each build supplied its own candidate. Both assemblies passed, with 16 inventoried files and 314 repository inputs. `kit payload-compare` passed. |

The owner retained `native-comparison.json`, both build directories and both
candidates under `out/relink-20261031-1151` in the PR #13 Windows worktree.
This records the owner's terminal report. The raw JSON is retained locally.
The linker version was not captured in the supplied output.

This closes the same-checkout Windows comparison that failed at PR #11.
The [PR #14 offline installation](#pr-14-windows-offline-installation)
subsequently packaged this pair using the private test recipe.

## Windows x64 release relink after the PR #11 comparison

The [PR #11 native report](validation-pr11-native-comparison.json) records
identical generated CLI Rust and two different 145,153,536-byte EXEs. The
following experiment isolates the final link. It ran on x86-64 Linux with
Rust/Cargo 1.96.0, the Windows MSVC standard library and bundled LLD 22.1.2.

A small Cargo package used the repository's native `build.rs`, both binary
target names and the release settings for debug info, stripping, incremental
compilation and LTO. Two distinct paths supplied byte-identical generated Rust
to the same target directory, forcing two rustc links. The Windows fixture
used a freestanding entry point with panic abort and no SDK libraries.

| Final Kit link | Result for the two complete PE files |
| --- | --- |
| Original build script | Different hashes and PE timestamps |
| `/INCREMENTAL:NO /Brepro` | Different hashes with embedded CodeView PDB identities |
| Also `/DEBUG:NONE` for a no-debug release | Byte-identical, 2,048 bytes each, SHA-256 `2a152c92e07eadec8ed557f3f84808d31c81081a2296febe2984c30cffcde11c` |

Changing the fixture's compiled return value changed the EXE hash. Windows
development builds, a release with debug info enabled, and the generator
with or without the native CLI feature retained CodeView records and received
none of the three link arguments. Both Linux fixture binaries built and ran.
The matrix comprised eleven Windows cross-links and two Linux builds.

Rust 1.96's MSVC linker driver requests a PDB even for the stripped Kit. A
separate LLD experiment kept the object bytes, working directory and output
path fixed. Changing only the object's input directory changed the PDB GUID
and EXE under the two-flag configuration. [Rust linker implementation](https://github.com/rust-lang/rust/blob/1.96.0/compiler/rustc_codegen_ssa/src/back/linker.rs)

The subsequent [native Windows run](#pr-13-native-windows-result) exercised
both build paths with the full Kit and compared the assembled payloads.

## Follow-up verifier encoding and download startup requirements

This follow-up starts from PR #11 merge
`e322fa271a760e0a5b18e56b6343a22d5f98a120`. It corrects the PowerShell verifier's
text decoding and the Topaz download archive's first-run dependency admission.
The canonical compiler, public runtime crates, native adapter and compiler
regeneration inputs are unchanged.

### Verifier encoding regression

The unmodified verifier accepted all five BOM variants of the same inventory
that the POSIX and actual Topaz verifiers rejected. The original six-encoding
matrix used eighteen invocations. Ordinary UTF-8 passed everywhere, while
UTF-8 with a BOM and both byte orders of UTF-16 and UTF-32 exposed the mismatch.
No file hashes or source relationships were changed for this reproduction.

`File.ReadAllText` detects a BOM even when supplied with a strict UTF-8 encoding.
The corrected helper decodes the actual bytes with strict UTF-8 instead.
The first correction exposed a second issue because culture-sensitive comment
recognition ignored the preserved BOM before the first comment. Comment-prefix
recognition now uses ordinal comparison. The intermediate failed case is
retained separately rather than counted as a successful verification.

The final matrix passes all twenty-seven expectations. It covers the six
original encodings and three malformed UTF-8 forms across PowerShell, POSIX
and the actual native Kit. Normal UTF-8 passes, and the eight invalid variants
are refused consistently. PowerShell ran on Linux; this is not Windows
PowerShell 5.1 execution evidence.

The before, intermediate and final reports are retained in scratch directory
`corekit-verifier-encoding-xlw5rf14`. Its original inventory was restored.
The final report is 4,359 bytes with SHA-256
`3483929fa31f8bc88ae8d3aedbeee67eeaa8dfd04a15fe75d814df676845b0d5`.
The public .NET behavior is documented in
[File.ReadAllText](https://learn.microsoft.com/en-us/dotnet/api/system.io.file.readalltext?view=netframework-4.8.1).

### Download startup admission

An isolated fixture copied the host's `libm.so.6` under its declared runtime
directory, classified that actual import only as a bundled runtime, and
refreshed its metadata with the existing Kit. The original Kit completed
assembly, comparison, packaging and explicit promotion. That reproduction
passed six commands and forty-one assertions. It demonstrates that the old
packaging rule admitted a dependency the executable-only first stage cannot
supply from the archive.

The corrected archive preparation inspects the actual executable imports after
all size preflights and binds the image size and digest to its verified member.
Each startup import must be in the assembly's system dependency list. An import
satisfied only by a runtime companion is refused before archive output. This
also applies when reconstructing an archive during explicit installed-payload
checking. General assembly and promotion retain their existing behavior.

The completed follow-up regression passes sixteen commands and eighty-eight
assertions, with fourteen successful commands and two expected refusals.

- The actual bundled-only `libm.so.6` import is refused before package output.
  Assembly, comparison and explicit promotion still succeed and retain the
  runtime file.
- The preserved PR #11 Kit packages and installs that same new candidate in
  the isolated upgrade fixture. The new Kit then refuses `kit install --check`
  with the startup-dependency diagnostic and leaves the installation unchanged.
- A separate candidate with an unused runtime companion packages normally.
  The actual POSIX `install-core-kit --archive` installs it, and explicit
  installed-payload checking succeeds.
- Both fixtures audit complete member bytes, modes, manifests and source
  identities. The real checkout inputs and supplied build artifacts retain
  their bytes, modes and modification times.

The before and after reports and compact logs are retained in scratch directory
`corekit-followup-startup-regression`. The after report is 70,168 bytes with
SHA-256 `8cab937139c6cc5a61aeba1fda4beba725c4b667223b8179033dbdf5eea0fbbf`.
The original fixtures remain under `/tmp/corekit-startup-dependency-before`
and `/tmp/corekit-startup-dependency-after`. Approval was enabled only in these
private fixtures. No public recipe, catalogue or release asset was changed.
The cases use one supplied build and do not establish independent reproduction.

### Follow-up build and ordinary smoke

The public core compiled the updated Topaz CLI and Rust/Cargo 1.96.0 linked
the Linux x86-64 executable. This reused the existing public core cache and
core-only opt-level=0 development override, with one Cargo job. It does not
establish a default-profile or independent native release build.

| Follow-up artifact | Bytes | SHA-256 |
| --- | --- | --- |
| Compiler request | 252,002 | `295b184e3cae3071a90e24c28e06c7486f3cda55be7afcdcbf9c8b7029f843d8` |
| Compiler response | 2,133,812 | `67b69a7594a3b3d28290d804fd40eb4199417e27281539f44fc3dd1f9b93b346` |
| Generated CLI Rust | 2,102,489 | `3197d55a01d057cc3e7c0ad48a016f239289f6ae4a26d2a144c5313065da1b4f` |
| Native executable | 103,982,592 | `a1de499fc573bbde76f74ca5c2adf2e342d57b01f7515006c5ff6beabfa570fe` |

The retained build directory is `out/core-kit-followup`. Ordinary repository
smoke passed with this executable, covering version/help, hashing, image
inspection, source/project checking, request replay, Rust emission, rejected
inputs, CLI maintenance checks and payload verification. Its logs remain in
`out/core-kit smoke.ogvbpV`. The unchanged native application build path and
native adapter unit tests were not repeated in this follow-up. Their PR #11
results are recorded separately below.

Both repository verifiers pass against the refreshed input records. The
27-module descriptor, 58 request facts, 50 canonical source pairs, 62 snapshot
files, 57 additional byte-verifier inputs and 314 payload paths are unchanged.

### Remaining native gate

The supplied Windows development recipe remains unapproved and still records
the observed `vcruntime140.dll` import. The follow-up does not remove that DLL
dependency, change Windows compiler flags or establish availability on a host
without developer tools. The native handoff adds a clean-host first-run gate
using the existing built archive and source checkout. The original Windows
executable mismatch and retained PR #8 evidence remain unchanged.

## Source checkout and optional release installation

This transition starts from the owner's diagnostic commit
`408313aea0240f595089db616888229e542c6bf7`. The public compiler, its generated
Rust, source manifest, runtime crates and seed inputs remain byte-identical.
Platform executables are packaged as optional named GitHub Release assets,
with installation policy in Topaz and bounded first-stage OS adapters.
No core self-regeneration was repeated.

### Windows PR #11 verification

The merged PR #11 revision `e322fa2` was checked out in a clean worktree on
Windows with Rust and Cargo 1.96.0. Ordinary bootstrap, native smoke and the
Topaz `kit build` path all passed. The generated CLI Rust from bootstrap and
`kit build` matched byte for byte at 2,091,722 bytes with SHA-256
`e3210ec170ea533e3e40f38e7c0b726203616db1893a12fe929f9d649c899cc8`.

The two newly linked Windows executables were both 145,153,536 bytes but did
not match. The bootstrap executable has SHA-256
`d015c7ee6509cbaf84cc4e3c6a530dc9cf4bb27491bd8efb0d37d0acacd47477` and the
`kit build` executable has SHA-256
`a0c4650260f436b743e7ba24f305aadd40d377c0121a2214e0f3bce187956b71`.
Their `.text`, `.data`, `.pdata` and `.reloc` section hashes matched, while
`.rdata`, the PE timestamps and PDB GUIDs differed. The complete bounded
report is [validation-pr11-native-comparison.json](validation-pr11-native-comparison.json).

The Windows development payload candidates assembled successfully, but
`payload-compare` rejected them because their recorded inventories differed.
The development recipe remains `releaseReady: false`, so no release archive
was created and no installation result is claimed. The installer help entry
point itself returned successfully.

The updated descriptor maps 27 Topaz CLI modules and the initial request has
58 source facts. The byte verifier covers 50 canonical source pairs, 62
original snapshot files and 57 additional inputs. The payload input list has
314 paths. The download catalogue is excluded from its own input identity;
installer and launcher code are included. Both inventory admission paths
reject an attempted catalogue self-reference.

### Final executable and source checks

The public core generated the final CLI Rust and Rust/Cargo 1.96.0 linked the
native Linux x86-64 Kit. This run retains the existing core-only opt-level=0
development override, one Cargo job and the public dependency cache. It does
not establish the unchanged default optimized release profile.

| Transition artifact | Bytes | SHA-256 |
| --- | --- | --- |
| Compiler request | 250,902 | `a9eb8371afd2b652bbbf98d95e6ffb43f414baec42ea22c5a3219a5f2333bf8b` |
| Compiler response | 2,122,893 | `ea1ade8b744d2de852efdf0839a8d78493816f4c8556a571072827c8f122ef02` |
| Generated CLI Rust | 2,091,722 | `e3210ec170ea533e3e40f38e7c0b726203616db1893a12fe929f9d649c899cc8` |
| Native executable | 103,922,856 | `9aba454efd28b56d3fddd5c3f94f900085d59a8d43964bf26b16439e1117d60b` |

The retained build directory is `out/core-kit-release-downloads-final`.
The full repository smoke passed with `--native-build`, including source and
project checks, saved request replay, error handling, CLI maintenance checks,
payload verification and both native application builds with exact output.
Its logs remain under `out/core-kit smoke.f2kPiB`.

All seventeen native adapter unit tests passed. Three new tests exercise
sibling-directory activation, preservation of existing destinations, invalid
sources, different parents and link rejection. The earlier mechanical file,
mode and rooted inspection tests also pass. Both the POSIX byte verifier and
PowerShell semantic verifier pass against the final inventories.

### Installer adapters

The POSIX, PowerShell and actual Topaz catalogue readers agree on the same
46 selection and refusal fixtures. They cover the supported target set,
sorting, duplicate targets, byte syntax, field counts, fixed tags, repository
names, file names, sizes, hashes and catalogue symlinks. Missing native rows
fail before a network request or cache write.

Controlled first-stage checks use a pinned inert executable fixture. They
cover bounded archive and executable extraction, size and hash refusal,
temporary cleanup, prior-version preservation and the fixed GitHub URL.
POSIX launcher checks preserve empty arguments, spaces, quotes, Korean text,
newlines, standard streams and exit status. PowerShell checks cover native
argument quoting, exact receipts, file identity and binary-safe tar output.
The tar-output fixture contains every byte value across 525,568 bytes; short,
oversized and invalid-archive outputs are refused.

These PowerShell checks ran under PowerShell 7 on Linux. They do not establish
Windows PowerShell 5.1, Windows CMD or macOS execution. No public release asset
was downloaded during this validation.

### Actual packaging and installation regression

The final executable passed 108 product commands and 358 assertions in private
fixtures. Eighteen commands expected success and ninety expected ordinary
refusal. Direct Topaz commands used PATH empty; OS-wrapper checks used the
ordinary system utilities. The fixtures contain the 314 declared source
inputs and the four actual build artifacts recorded above.

- Both development-recipe packaging and promotion are refused. Only private
  test fixtures enable `releaseReady` to exercise the successful path.
- Each assembly contains exactly seventeen regular files. Complete bytes,
  file modes, notices, generated Rust and both manifests are checked by an
  independent reader.
- Repeated packaging produces byte-identical archives, catalogue rows and
  package receipts. Independent gzip, tarfile and system tar readers verify
  the archive structure, all member bytes and the extracted executable.
- Explicit promotion preserves the candidates and refuses existing output
  directories. Nothing selects a destination under `dist`.
- Individual 256 MiB and cumulative tar-size limits reject oversized sparse
  candidates before reading their bodies. The catalogue cannot be introduced
  into its own payload or source-verifier inventory.
- Actual installation verifies the full archive and native payload, writes
  the exact receipt, and activates the complete version. Explicit checking is
  read-only. Reinstallation preserves version bytes, modes and modification
  times, while the ordinary launcher remains usable after source edits.
- An altered receipt, extra file or changed executable is refused. Coherently
  rewriting an installed companion and both mutable local manifests still
  fails the canonical archive identity check against the unchanged catalogue.
- Eleven malformed tar and seven malformed gzip fixtures are refused. These
  include traversal, hidden or duplicate names, case aliases, order, links,
  modes, padding, headers, end blocks, CRC, length, timestamp and trailing data.
  Wrong outer archive size and digest are also refused before staging.
- Existing locks remain untouched. A staged invalid native image fails before
  activation, retains its diagnostic stage without a receipt, releases the
  owned lock and preserves the older complete installation.
- The actual POSIX offline wrapper installs the real product into a separate
  matching checkout whose path contains spaces and Korean text. Native source
  checking, full installed verification, earlier-version preservation and
  symlink-cache refusal pass.

The test archive contains 106,742,784 tar bytes and 106,750,947 received bytes.
Its SHA-256 is
`dc126237cb7eef29bef5e69d344b7b5cfa532fe77254251a6a91cf696520eb15`.
The public stored-DEFLATE codec preserves deterministic gzip framing; these
measurements do not claim compression savings. The archive is a private test
artifact and is not a published release.

Two candidates in this run were assembled from one supplied build. Their
agreement establishes deterministic packaging and installer behavior, not
independent native executable or release-payload reproduction. The source
commit in the test catalogue is explicitly a publisher declaration. The
separate input-set digest binds the actual checked source bytes.

The completed report is 163,125 bytes with SHA-256
`17aa28a452057205c8b1856529cf5175efea25c800bda4ee2e83a2cbc2385e55`.
It and the compact logs are retained in scratch directory
`corekit-download-installation-regression`. The original fixture remains at
`/tmp/corekit-download-regression-4`. All real repository inputs and supplied
build artifacts retained their bytes, modes and modification times.

### Corrections and evidence limits

An earlier actual packaging run exposed shared Topaz array identity in the
candidate comparison. Appending the manifest to an alias changed the recorded
file array and made the subsequent native check fail. Comparison and packaging
now copy records into a fresh array before appending. The final executable
above includes that correction. The earlier failure is retained separately.

The Windows development recipe records exactly the ten imports in the owner's
PR #8 report. It remains `releaseReady` false and establishes neither current
Windows execution nor a minimum supported Windows version. The retained pair
has different hashes in all five recorded sections, including `.text`; the
cause remains unproven. The new installation path does not claim to fix that
native executable mismatch.

The real catalogue remains header-only and both platform development recipes
remain unapproved. No executable was committed to Git and no Release asset,
catalogue row, repository migration or public distribution was published.
Scratch regression drivers do not add Python or another runtime to the
product, bootstrap, installer or maintainer command path.

## Earlier PR #10 distribution assembly and native image inspection

This section preserves the earlier artifacts and former Git-payload policy.
The release-download transition described above supersedes that distribution
destination and size rule. Historical command results retain their original
meaning and are not presented as runs of the new installer.

This change starts from PR #9 merge
`78430a847ce3d1be6af008cdc4b60776618c6271`. Four additional Topaz modules own
native image inspection, bounded binary comparison, recipe admission and
complete candidate assembly/comparison/promotion. Rust adds mechanical file
metadata and mode operations. The canonical compiler source, generated Rust,
manifest and regeneration input are unchanged. No core self-regeneration was
repeated.

The working maintenance command refreshed the descriptor and initial request.
The newly linked Kit then refreshed its own inventory to include the six
distribution inputs. The final descriptor maps 23 modules, the initial request
has 50 facts, the byte verifier covers 50 canonical source pairs, 62 original
snapshot files and 52 inputs, and the payload path list has 303 entries.

### Actual executable and ordinary smoke

The mechanical source-bootstrap generator emitted the CLI Rust through the
public core. Rust/Cargo 1.96.0 linked the native x86_64 Linux executable using
the existing core-only opt-level=0 development override and one Cargo job.
The first native link completed in 2m14s. This reuses the earlier public core
cache and does not validate the unchanged default optimized core profile.

| Distribution artifact | Bytes | SHA-256 |
| --- | --- | --- |
| Generated CLI Rust | 1,698,908 | `dc628decdfdf76d04ef06938a7601867e6c178ee33502329cbebb4a23a0f9646` |
| Native executable | 101,313,392 | `8f297e083628e3e9e07aef14586f17b0f1d0c8463f6d6e6fd3d957a87a35c103` |

The repository smoke passed with --native-build. It covered the actual ELF
image report, existing source and project checks, request replay, error and
output handling, the current snapshot/callable check, byte inventories and
both native application builds with exact output. Logs remain under
`out/core-kit smoke.Qahlgz`.

All fourteen native adapter unit tests passed, including regular-file size
and mode observations, symlink and nonregular rejection, copy preservation
and mode normalization. On Unix the observation includes special permission
bits, and a setuid fixture was observed as 4755 before normalization to 0755.
The Windows-specific read-only-attribute test is not executed on this host.
Both the POSIX byte verifier and PowerShell 7.6.9 semantic verifier passed.
That PowerShell run is on Linux, not Windows PowerShell 5.1.

### Native image regression

The actual executable passed 72 image cases, comprising ten successful
inspections/comparisons and 62 expected ordinary refusals. Every child used
the selected native Kit by absolute path with PATH empty. Synthetic images
were never executed.

- Synthetic PE32+ x86_64 and arm64 images exercised normalized dependencies,
  timestamps, PDB GUID/age/path and section hashes.
- Synthetic thin Mach-O x86_64 and arm64 images exercised exact dylib names
  and supported loader metadata, including diagnostic @rpath observations.
- Invalid ranges, truncated images, unsupported machines, malformed imports,
  delay imports, oversized tables and invalid debug/load commands were refused.
- Identical images and header, section and trailing-byte differences produced
  the expected complete-byte result. Difference samples stopped at 16 offsets.
- A sparse 512 MiB plus one byte file was refused before reading its body under
  a 384 MiB address-space limit. It returned ordinary exit code one with the
  size diagnostic in approximately 0.006 seconds.

An independent streaming reader checked the actual ELF's complete digest,
all 29 file-backed section hashes and DT_NEEDED list. The reported dependencies
were ld-linux-x86-64.so.2, libc.so.6, libgcc_s.so.1 and libm.so.6. The recorded
interpreter was /lib64/ld-linux-x86-64.so.2. These structural observations do
not establish a minimum OS or native execution on the other recognized targets.

The complete report remains in scratch directory
`corekit-image-report-paj2dzb5`, with SHA-256
`80c9ac68ecffe0dbe27cf7da8537bf9a7e1e188c38ff79d0f450c12c7353472f`.
An earlier default-sandbox invocation failed before reaching the parser because
/proc/self/exe was unavailable. Its separate report was retained. The successful
run used the native execution environment and required no product correction.

### Complete candidate and promotion regression

The actual Kit passed 29 product commands and 149 assertions in private
fixtures containing the 303 declared inputs and its four real build artifacts.
Nine commands expected success and twenty expected refusal. The first product
run completed without a source or fixture correction.

- Each normal assembly contained exactly 17 regular files, including the
  executable, generated CLI Rust, thirteen notice/component companions and
  both manifests. Complete bytes, required modes and all recorded repository
  inputs were checked independently.
- Equal candidates compared successfully. The same physical directory and
  an alias of it were refused. Comparison preserved paths, bytes, modes and
  modification times.
- Missing notices, missing required mappings, noncanonical recipes,
  unclassified runtime imports, stale sources/requests/recipes and a mismatch
  between the successful compiler response and generated Rust were refused.
- Oversized executable, unexpected and generated-source files were refused.
  A substituted ELF interpreter with the correct basename but wrong absolute
  path failed admission. Missing execute permission and setuid mode failed.
- The checked-in development recipe refused promotion. Only inside a private
  fixture, releaseReady was enabled to exercise the implemented native path.
  The candidate passed its version, source, project and input checks. Promotion
  retained both manifests verbatim, copied all candidate bytes and required
  modes, preserved both candidates and refused an existing dist destination.
- The promoted fixture passed version and source checking through its root
  launcher. Direct product commands used PATH empty; launcher checks used only
  /usr/bin and /bin for the shell's ordinary OS commands.

All original repository inputs and supplied build artifacts retained their
bytes, modes and modification times. The report and logs were copied from the
private fixture to scratch directory `corekit-distribution-payload-regression`.
The report is 85,874 bytes with SHA-256
`7d164216d3c8eb4db7406256e36a911274212efd5dca518d88bcc3340a117223`.
The original private fixture and its logs were retained as well.

These candidates were assembled from one supplied build. The checks establish
the assembly, rejection, comparison and promotion behavior, not an independent
build history. The test-only approved policy was never applied to the real
checkout. Its development recipe remains releaseReady false and dist remains
without platform binaries.

### Selected output path comparison

The generated CLI Rust was copied unchanged to a second output directory whose
path included spaces and Korean text. The native target was linked again in
the same checkout with the same Cargo cache and flags, selecting that second
source file. The second native link completed in 2m06s.

Both complete executables matched directly, with the 101,313,392-byte identity
above. The generated Rust also matched directly. The retained record is
`out/core-kit-distribution-output-path-comparison.json`. This checks the new
build-script staging at a fixed OUT_DIR/generated_cli.rs path. It is a
same-checkout, shared-cache native path test, not independent core compilation,
a fresh Topaz kit build, or a Windows reproduction result.

### Distribution evidence limits

The checked-in recipe is a Linux development recipe and cannot promote a real
release. Default optimized platform builds, complete independent release
payload reproduction, authoritative component-policy completion and native
Windows/macOS validation remain separate work. At that validation point the
retained PR #8 Windows pair had not been inspected here and exceeded the then
selected ordinary Git limit. The later owner report below supplies the bounded
inspection. Optional Release assets now replace the old Git binary policy;
the Windows executable byte mismatch remains unresolved.

### Windows PR #8 executable comparison

The retained Windows pair from PR #8 was inspected after PR #10 was merged.
The complete machine-readable result is
[`validation-pr8-native-comparison.json`](validation-pr8-native-comparison.json).
Both PE files are `x86_64-pc-windows-msvc`, 131,391,488 bytes, and have the
same imported DLL list, but they are not byte-identical.

The PE timestamps are `1793396347` and `1793396526`, and the PDB GUIDs differ.
The `.text`, `.rdata`, `.data`, `.pdata`, and `.reloc` section hashes all
differ, so the mismatch is not limited to headers or debug metadata.

The recorded SHA-256 values are
`ea2b4795c6ebdd9e5dffa29e30a21b8e7a44666c5970395657e96baea5238054` and
`0e2d3d05a2498f3fb4eeeb3cc421977f58d370d2cca413c15810ffeadeadaad3`.
Windows executable byte reproducibility remains unresolved.

Regression orchestration used scratch-only Python drivers. No Python program
or required scripting runtime was added to the repository, bootstrap, product,
maintenance or distribution path.

## Owner-reported Windows project and Kit-build validation

On 2026-10-31 KST, the owner supplied the completed Windows verification
transcript for PR #8 merge
`b6a97245189365f93801f9e996c3c3152bcd3e3e`. The local agent used a detached
worktree at `D:\Projects\topaz-note-pr8-verify` and retained that worktree,
its Cargo cache and output files after verification.

The agent reported successful ordinary `bootstrap.cmd` and
`core-kit\smoke.cmd --native-build` runs using the pinned Rust/Cargo 1.96.0
offline build path. The expanded smoke covered project `--root` resolution,
checking and building, saved request replay, payload verification, and native
sample and project execution. The smoke logs remain at
`D:\Projects\topaz-note-pr8-verify\out\core-kit smoke-17111-2065`.

The bootstrap executable then completed `kit build`. The resulting Kit passed
`--version`, `check --root` against the bundled project and `kit verify`.
The generated CLI Rust from bootstrap and Kit build matched completely using
`fc /b`. Its identity also matches the Linux PR #8 generated source recorded
below.

| Reported artifact | Bytes | SHA-256 |
| --- | --- | --- |
| Generated CLI Rust from both paths | 690,692 | `807d2c17195263f90a318c77a997d9b314f23739d4237576b3b3c30137dc524a` |
| Bootstrap Windows executable | 131,391,488 | `ea2b4795c6ebdd9e5dffa29e30a21b8e7a44666c5970395657e96baea5238054` |
| Kit-build Windows executable | 131,391,488 | `0e2d3d05a2498f3fb4eeeb3cc421977f58d370d2cca413c15810ffeadeadaad3` |

The two executables have the same length but different bytes. Their `fc /b`
comparison failed. An attempt to print the full executable differences was
stopped. The subsequent bounded owner report identifies different hashes in
all five sections, but does not establish why. Neither output-path differences
nor linker metadata are treated as a demonstrated explanation.

This is owner-reported execution evidence. The Windows executables, complete
raw logs, inherited build configuration and linker/SDK identity were not
independently inspected on the Linux validation host. The generated-source
match compares two paths in one worktree and does not establish independent
native executable or complete payload reproduction. The unequal executable
identities are retained as a failed comparison, not a successful reproduction.

Canonical core self-regeneration was deliberately not repeated. This report
covers PR #8 project and Kit-build behavior. It does not establish Windows
execution of the later PR #9 maintenance command or its new native operations.

## Topaz CLI maintenance follow-up

This follow-up starts from PR #8 merge
`b6a97245189365f93801f9e996c3c3152bcd3e3e`. It adds `kit refresh --check` and
`kit refresh`, with maintenance policy in three Topaz modules. Rust gains only
exclusive directory creation and empty-directory removal. The canonical core,
public runtime, source manifest and compiler regeneration input are unchanged.
No core self-regeneration was repeated.

The refreshed CLI descriptor has 19 modules and its initial request has 42
facts. The verification inventory covers 50 canonical source pairs, 62 original
snapshot files and 42 inputs. The payload path list has 293 entries. A working
Topaz maintenance executable produced the refreshed metadata in the actual
checkout, including the final source changes. No repository generation script
or additional scripting runtime is needed to repeat this operation.

### Generated CLI and executable

The mechanical source-bootstrap generator consumed the refreshed descriptor
and initial request and emitted the final CLI Rust. The installed Rust/Cargo
1.96.0 toolchain linked it for x86_64-unknown-linux-gnu using the existing
core-only opt-level=0 development override and one Cargo job. The final native
link completed in 1m06s. The checked-in default core profile was not lowered.

| Final maintenance artifact | Bytes | SHA-256 |
| --- | --- | --- |
| Generated CLI Rust | 1,074,820 | `1021a82f9091486fae057b7ba919ff45813e3a5aa7334356c69a227c128e2a90` |
| Native executable | 97,236,240 | `c4b4dfd255d7d137714c1f89a16403a3b1439aa929ffa61b4a6b8cdd54d829e9` |

The final ordinary smoke passed, including the newly added read-only refresh
and callable check, source and project checking, saved-input replay, exact
output and payload recording and verification. Its logs remain under
`out/core-kit smoke.XtgNcH`. The application native-build recipe is unchanged
from PR #8 and its expensive native smoke was not repeated for this change.

All ten native adapter unit tests passed. The added cases exercise exclusive
creation, existing entries and missing parents, empty-directory removal,
nonempty directories and symlink refusal. The POSIX byte verifier and the
PowerShell 7.6.9 semantic verifier also passed on the final metadata. PowerShell
execution here is on Linux, not Windows PowerShell 5.1. The Windows handoff
and changed POSIX smoke received syntax checks.

### Maintenance behavior and real rollback

The final executable completed 42 product commands and 217 assertions in
isolated fixtures containing the 293 declared repository inputs. Every product
command used an empty PATH. These checks exercised actual Topaz policy and
the public compiler, without a substitute maintenance implementation.

- Read-only current and stale checks, a no-change update and idempotence.
- Edited source refresh, imported nested-module discovery and deletion of a
  previously mapped module. Snapshot source text, complete directory facts,
  inventories and hashes were checked against the resulting files.
- Preserved recorded resource budgets, callable metadata and generated Rust
  location. The transient callable guard did not enter the saved snapshot.
- Actual CLI signature changes while leaving the descriptor unchanged. A
  seventh required or defaulted argument and a different return type produced
  TPZ5001 at the guard's function assignment. Removing the export produced
  the expected public-import diagnostics.
- Type errors, missing imports, changed protected inputs, invalid descriptors,
  symlink entries, an existing lock and unsupported command arguments.
- Byte-exact backup contents and receipt hashes. Read-only and rejected
  operations preserved fixture paths, bytes, permissions and modification
  times, including the existing lock and symlink targets.

The detailed report SHA-256 is
`d72cf43709af22630b0e6ca2b8dbf5676f8d03b87c113f54f22c37e1c08a5fee`.
It and the command logs were copied from the private fixture into the scratch
`corekit-refresh-final-regression` directory after completion.

A separate real filesystem failure exercised rollback on the same final
executable. A private fixture ran as an unprivileged account, with the second
metadata file's parent directory made unwritable. The first replacement could
complete and the next failed with permission denied. The command returned one,
restored all four metadata files byte for byte, saved a rolled-back receipt,
released its lock and left no copy temporary files. No product failure hook
or fake native callback was used. The receipt and before hashes remain in the
scratch `corekit-refresh-final-rollback-result.json` record and companion log.

### Earlier attempts and limits

An earlier maintenance executable passed 37 commands and 196 assertions in a
private temporary fixture, before the actual callable-type guard was added.
Its generated Rust was 1,052,888 bytes with SHA-256
`e67608013fd3eade1dc5213f699b17c46f5399d859cdf3df72e29c579990cc1c`.
The corresponding 97,075,872-byte executable had SHA-256
`6418c0599528d0b7bf4abdb67ede41ab4773e30a6a6080577c8b36535c9b29b7`.
Regeneration from metadata written by that executable produced identical
complete CLI Rust by direct comparison.

The first run of that earlier suite in the shared scratch directory stopped
when a lock directory was unexpectedly present after the previous successful
operation and its absence check. The cause was not established. The same
executable, driver and declared input hashes completed the full suite in a
private temporary directory without a code change. The failed record was kept.
Mutation regression uses private temporary fixtures for the final candidate.

The first callable-guard candidate rejected its valid entry with TPZ3014
because an exported check function referred to a private type alias. Exporting
the temporary alias corrected that source error. The corrected guard compiled
through the public core before the final executable was regenerated and linked.
The guard and its bounded temporary compilation allowance are never recorded
in the actual CLI snapshot.

The retained build records are under `out/core-kit-maintenance-final` and its
adjacent generation, linking, smoke and verifier logs. Additional regression
drivers are scratch-only test instruments. Their Python orchestration does not
become a repository, bootstrap, user or maintenance dependency.

These results establish execution of this maintenance change on the recorded
Linux development configuration. They do not establish an independent rebuild
of this executable, a default optimized platform release, Windows execution
of the new operations or a complete distribution payload. The owner-reported
Windows result above covers the earlier PR #8 revision separately.

## Owner-reported Windows payload validation

On 2026-10-31 KST, the owner supplied the local agent's completed verification
transcript for PR #4 merge
`f2031f09f967511fdf26d1e86f8b8868a6a39d9f`. The agent used a clean detached
worktree at `D:\Projects\topaz-note-pr4-verify` and reported successful
`bootstrap.cmd` and `core-kit\smoke.cmd --native-build` runs.

The ordinary bootstrap used the pinned Rust/Cargo 1.96.0 offline path, generated
the Topaz CLI and linked the native Kit. The smoke run covered version, help,
exact hash output, source checking, request creation and reading, Rust emission,
type-error rejection, the then-unsupported `--root` rejection, input verification,
payload recording and comparison, extra-file rejection, and native sample
build and execution. The payload comparison reported one payload file and
277 repository inputs.

This revision includes the native Cargo manifest relocation and exact `fc /b`
output assertions from PR #3, as well as the payload commands from PR #4. It
therefore extends the earlier Windows execution record to those changes.

| Artifact | Reported bytes | Reported SHA-256 |
| --- | --- | --- |
| `generated_cli.rs` | 332,177 | `1c268f4fcae5e8fb2ec4534deecd557a0926a4b5247a36be465fe90208907895` |
| `topaz-core-kit.exe` | 127,672,320 | `cfcc1ef1c45be428b8cb9a04103c547cc31c53a369ffe6729d48925ced80cd05` |

The generated CLI identity matches the Linux payload-command build recorded
below. The Windows executable digest identifies one artifact. It does not
establish an independent Windows rebuild or complete payload reproduction.

The owner reported the smoke logs retained at
`D:\Projects\topaz-note-pr4-verify\out\core-kit smoke-31867-23936`.
The transcript shows unsuccessful attempts to copy those logs into the OPS
archive. No successful secondary archive, complete raw log set, linker/SDK
identity or inherited build configuration was supplied for independent
inspection. Preserve the original worktree and its Cargo cache while using
this result for follow-up work.

Canonical compiler self-regeneration was deliberately not repeated. The
earlier owner-reported match still covers the unchanged canonical inputs.
This run adds Windows bootstrap and product execution evidence without
claiming a new core comparison, minimum OS/ABI validation or a published
distribution.

## Project input and Topaz Kit build

This change starts from integration commit
`76e0abf566426e345d1671ba9ecf54ee699390f5`. The canonical compiler source,
generated core Rust and compiler seed input have no Git changes. Its source
set remains
`sha256:17d598ebcb0dea65195aba8c0b3ab20f94003a7f343714f7b1df25d790cdacd5`.
The 13,077,120-byte canonical generated Rust still hashes to
`5f32cee07a1adea1c67ba8ac8fbd4f3ee4051f019a7856fcdb228740a1d4b8b1`.
No canonical self-regeneration was run for this change.

The CLI now has sixteen source modules and 36 initial source/directory facts.
It uses the existing public TOML operations through an explicit `std.toml`
boundary. The verifier covers 50 compiler source pairs, 62 snapshot files and
38 inputs, and the payload dependency list contains 290 repository files.

### Build paths and a live source edit

Linux validation used the installed Rust/Cargo 1.96.0 toolchain and the earlier
core-only `opt-level=0` development override, with one Cargo job for the
ordinary build. The canonical core's default profile in Git was not lowered.
The updated native adapter passed seven unit tests. The complete Topaz CLI
compiled through the canonical core, and the ordinary bootstrap generated and
linked an executable successfully.

An actual `kit build` invoked from that executable then prepared current CLI
inputs in Topaz, called the canonical compiler, linked through Cargo and copied
the resulting executable. It did not invoke a shell bootstrap or a second
compiler implementation.

A later check deliberately left the embedded initial request unchanged while
editing the mapped `native_build.tpz` source to add the application profile
below. The initial request SHA-256 remained
`bd77aac9806cee5ce743c366b002987d6ece3efbbaa50255e5c1d73c932b71a2`
during that build. Its embedded source lacked the new setting, while the
materialized request contained the exact changed source. The real Kit build
completed, including its 44.75-second native link. This exercises source
materialization directly instead of only a no-change rebuild.

After refreshing the committed initial input and inventory, the mechanical
bootstrap generator ran again. Its complete generated Rust matched the Topaz
Kit-build output by `cmp`.

| Final ordinary candidate | Bytes | SHA-256 |
| --- | --- | --- |
| Generated CLI Rust from both paths | 690,692 | `807d2c17195263f90a318c77a997d9b314f23739d4237576b3b3c30137dc524a` |
| Native Kit executable | 94,545,688 | `4bc598d3d09ad923f40a9337f35f6ff363b5210279558f9ff84e622fdb4fb884` |

The native Kit package now strips symbol tables. Before that package-specific
setting, the preceding candidate was 105,112,216 bytes. The two candidates
also differ by the small application-profile source edit, so this is not an
exact same-source size benchmark. At that stage the measured Linux executable
fit below GitHub's ordinary 100 MiB file limit. The owner-reported Windows
PR #8 executables are 131,391,488 bytes each and exceed that former Git-payload
policy. The Release-download transition removes that distribution constraint.
No binary was committed by this change.
The [GitHub large-file documentation](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github)
describes that hosting limit.

### Project and Kit admission checks

Twenty-eight Kit admission scenarios passed on the preceding unstripped
candidate, SHA-256
`ab24287ee91de889d026f076b701827fabe95e253562d94f7fa2d2b4ac113e54`.
The scenarios covered failed or incorrect toolchain probes, duplicate metadata,
host mismatch, fixed probe environment and working directory, output markers,
symlink refusal, canonical identity and digest, explicit source and standard
mappings, duplicate or unused mappings, missing inputs and initial request
admission. Controlled subprocess substitutes establish rejection and ordering
policy. The real Kit build above establishes actual compiler and Cargo use.

Thirty-seven project scenarios resolved successfully on that same candidate.
Twenty-two cover graph and saved-input behavior, including Unicode and spaced
roots, local imports, relocation, closed replay after moving the original
project, deterministic snapshots, automatic parent creation, invalid manifests,
type errors, missing or invalid UTF-8 source, escaping links, inside aliases
and case collisions. Fifteen cover generated native programs and their
filesystem grants.

The native capability cases retained grants after moving the original source,
allowed the declared data reads and result writes, denied unrelated reads and
writes through read-only grants, refused an escaping data link and confirmed
that empty grants deny both operations. Generated capability source uses the
application's launch directory and contains no absolute source checkout path.
The first fresh Cargo attempts for these two applications failed as described
below. Unchanged retries in their own output directories completed, after which
the behavioral checks passed. These results are not first-attempt build passes.

The seven Rust adapter tests cover rooted inspection, missing path ancestors,
escaping links and file copying without source truncation. Source inspection
also confirmed the six-argument entry point, compiler/provider context boundary,
public runtime capability API and Windows path transport. This static Windows
review is separate from the owner-reported PR #4 and PR #8 execution above.

### Application archive failures and the explicit profile

Several fresh application builds initially failed with an archive error saying
that an object file could not be mapped because its length was zero. An older
Kit executable reproduced the failure in a fresh directory. The failure is
therefore not specific to the new captured toolchain metadata or project mode.
Unchanged retries sometimes completed successfully.

An isolated attempt and a later attempt after reducing file-cache pressure
also failed. Cgroup OOM counters did not increase during these application
probes. The host's older core-build OOM event remains the separate event
recorded below. These observations do not establish the archive failure's
root cause. Zero-length intermediate files also remained after successful
builds, so their mere presence is not a reliable failure diagnosis.

Two separate fresh controlled builds then used only the additional
`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1` setting. Both completed on their first
attempt, in 31.89 and 31.96 seconds, and both produced the same 1,781,176-byte
hello executable with SHA-256
`1696072c6148aed764a18ad9aa59dcf979ab6d0628f32475d37649cb95fcd180`.
Both executables returned the expected complete output. One controlled build
log was incomplete despite a recorded successful process result and retained
artifact, so it is not described as a complete raw log pair.

The generated application manifest now explicitly sets release
`codegen-units = 1`. This bounds code-generation parallelism in the application
workspace, including its runtime dependencies. It does not change the Kit or
canonical compiler workspace profile. No automatic retry was added.

### Final ordinary smoke

The final candidate above passed the complete official
`core-kit/smoke.sh <candidate> --native-build` gate without a
`CARGO_PROFILE_RELEASE_CODEGEN_UNITS` environment override. Its generated
application manifests supplied the setting. The two fresh application builds
completed on their first attempts in 31.73 and 30.51 seconds.

The smoke covered version/help, exact hash output, source check/request/Rust
emission, project resolution and saved input replay, automatic nested output
creation, expected type and missing-manifest failures, source verification,
payload record/verify and extra-file rejection. Both the standalone hello
program and saved multi-module project built and ran with exact fixture output.
Payload verification matched one executable and 290 repository inputs.

| Smoke application | Bytes | SHA-256 |
| --- | --- | --- |
| Standalone hello | 1,781,176 | `1696072c6148aed764a18ad9aa59dcf979ab6d0628f32475d37649cb95fcd180` |
| Saved multi-module project | 1,788,904 | `bd44c5a345641e0d7550e3c5581f8ca56d4550ff14b9e531de0a5cf813d047b8` |

The final POSIX byte verifier and PowerShell 7.6.9 semantic verifier both
passed. This is PowerShell execution on Linux, not Windows PowerShell 5.1.
Changed POSIX script syntax and Git whitespace checks also passed.

The ordinary smoke logs remain under `out/core-kit smoke.HJqGZo`, with build,
source-edit and artifact records under `out/core-kit-project`. The preceding
project and Kit regression drivers and their detailed results remain scratch
validation instruments. No Python, Node or jq dependency was added to the
repository's bootstrap, product, smoke or verification paths.

### Independent updated development candidates

The two earlier isolated build trees were updated to the same 290 declared
repository inputs. They have different-length checkout and external output
paths, including Korean characters and spaces, and separate Cargo homes.
Each reused only its own independently built public dependency cache. All eight
public dependency artifacts were compared before and after and remained
unchanged. No build artifacts were copied from one tree to the other.

Both builds used the documented POSIX source-path remapping recipe, the same
Linux host and Rust/Cargo 1.96.0, core-only `opt-level=0`, core-only
`codegen-units=256` and two Cargo jobs. The native Kit symbol-stripping profile
was part of the common checked-in inputs. CLI generation, native linking,
recording and candidate checks took 77.950 and 69.844 seconds.

| Item | Both updated builds |
| --- | --- |
| Native executable bytes | 94,545,224 |
| Native executable SHA-256 | `7c1b4b5835d208d5572d66b9780f0b73b0315c99a73238b6336bdae835915cf6` |
| Generated CLI Rust bytes | 690,692 |
| Generated CLI Rust SHA-256 | `807d2c17195263f90a318c77a997d9b314f23739d4237576b3b3c30137dc524a` |
| Candidate payload manifest SHA-256 | `0e8946e1040a383d8c469ec0d73e1efb02622ae32bf35273cbb99e7f4263f601` |

Each candidate contained the actual executable, repository LICENSE and NOTICE.
Direct `cmp` passed for all three files and for the resulting payload manifest,
which identified the same 290 repository inputs. Both copied candidates passed
source and project checks and actual payload record/verify commands against
their respective checkouts.

These three-file candidates exercise executable and inventory reproduction.
They are not complete platform notice bundles. This is same-host Linux
development evidence with independently retained caches, not a new cold core
build, default optimized release, cross-machine result or Windows comparison.
The ordinary candidate above uses a different path/profile recipe and is not
expected to have this pair's executable hash. The reproduction record and logs
remain in the scratch `corekit-project-reproduction` directory.

## Payload-command follow-up — 2026-10-31 KST

This follow-up starts from PR #3 merge
`ec296cbb978f3b9b577268604c2b713bbc8b8c04`. It adds two Topaz payload commands
and records the separately reported Windows core-regeneration result below.
Canonical compiler source, generated core Rust and the native Rust adapter are
unchanged. The CLI now has ten source modules and 24 source/directory facts;
the ordinary verifier checks 50 source pairs, 62 snapshot files and 27 inputs.

### Actual Linux execution

The existing core-only `opt-level=0` development configuration completed source
bootstrap, regenerated the CLI with empty resolver/checker diagnostics and
linked the native executable. The ordinary executable was 102,509,064 bytes,
with SHA-256:

```text
e30eca6e6019d0bc6cbd93d5f2938e4327540e48097f5cfeb3c82139eaee6cff
```

The first CLI compile exposed a Topaz `?.` tokenization error in a new chained
`Result` expression. Splitting it into two statements fixed the source before
the successful build. The final generated CLI Rust was 332,177 bytes, SHA-256:

```text
1c268f4fcae5e8fb2ec4534deecd557a0926a4b5247a36be465fe90208907895
```

The full `smoke.sh --native-build` passed, including the new actual-executable
payload recording/verification and expected extra-file rejection. It also
built and ran the native hello sample. Logs were retained under
`out/core-kit smoke.KWJ5Ru`. Both the POSIX recorded-byte verifier and the
PowerShell 7.6.9 semantic verifier passed the updated snapshot.

The isolated payload regression set resolved all 99 distinct scenarios:
19 expected successes and 80 expected rejections. Rejections required exit 1
and a relevant diagnostic. Coverage included malformed manifests, unknown and
duplicate JSON fields, omitted/reordered rows, changed sizes/hashes, strict
options, case aliases, Windows device names, symlinks, missing/extra files,
malformed input lists and the canonical/source hash gate. Two candidates made
from the same actual executable produced identical manifests after relocation
to different Unicode/space-containing paths; this check establishes inventory
determinism, separately from the independent builds below. Tiny opaque files
used for negative cases and target-name mappings are not platform execution
results.

The initial regression run passed 89 scenarios; ten later checks were affected
by reused scratch fixtures that had not been restored. Those ten were rerun
against fresh immutable fixtures. All 16 targeted rerun/setup/control calls
passed, without a product change; the earlier failure logs were retained.
The test drivers remain scratch-only development instruments. They add no
Python dependency to bootstrap, product use, verification or the repository.

### Independent development candidates

The two separate source exports and separate Cargo homes from the preceding
reproduction experiment were updated to the same final 277 input files. Each
build reused only its own previously independent public-core/runtime cache.
Those eight public dependency artifacts were byte-checked before and after and
remained unchanged. The CLI was generated and linked separately in each tree.
No artifacts were copied between the two build trees.

Both used the recorded POSIX path-remapping recipe, the same Linux host and
Rust 1.96.0, core-only `opt-level=0`, core-only `codegen-units=256` and two Cargo
jobs. These are development overrides, not the repository's default release
profile. Their complete native executables matched by `cmp`:

| Item | Both independent builds |
| --- | --- |
| Executable size | 102,512,152 bytes |
| Executable SHA-256 | `3467628e9232d4d71477b2601ce322f44f2ffb77863c93ad9147239ec37a50ed` |
| Generated CLI SHA-256 | `1c268f4fcae5e8fb2ec4534deecd557a0926a4b5247a36be465fe90208907895` |
| Manifest repository inputs | 277 |
| Recorded candidate files | Executable, `LICENSE`, `NOTICE` |
| `payload.json` SHA-256 | `767e1e2e70d47680f316dbea8da7bf9142ea05dc4697230fcf32abb431cde43d` |

Each candidate executable successfully recorded and verified its own manifest
and checked the hello source against its own checkout. Direct byte comparison
passed for all four candidate files, including `payload.json`. The three-file
candidate is an inventory/reproduction test fixture; its notices were not
assembled as a complete release notice bundle. Nothing was promoted to `dist/`.

`readelf` inspection of this Linux candidate records `/lib64/ld-linux-x86-64.so.2`
as its interpreter and dependencies on `libgcc_s.so.1`, `libm.so.6`, `libc.so.6`
and `ld-linux-x86-64.so.2`. Required symbol versions include `GLIBC_2.39`.
This artifact must not be advertised for arbitrary older glibc installations;
minimum-platform release work remains necessary.

The new Topaz commands and updated CMD smoke were subsequently run on Windows,
as recorded in the owner-reported PR #4 result above. The previously reported
Windows core comparison remains useful evidence for the unchanged canonical
inputs; no additional self-regeneration was run in either payload follow-up.
The source/bootstrap and payload validation do not yet establish a default
optimized, cross-machine or complete published distribution.

## Scope and inputs

Validation started from the complete public checkout at merge commit
`4c44896ffdd14e83061e0b2a42cedc7196560b7f`, which contains the first Core Kit
repair. The follow-up checks cover script failure handling and build-log
improvements. The canonical compiler Topaz source and generated Rust were
not modified. This record reports development execution, not a release or
whole-Kit reproducibility certification.

| Item | Recorded value |
| --- | --- |
| Host/target | Linux, `x86_64-unknown-linux-gnu` |
| Memory constraint | 8 GiB |
| Rust | Official 1.96.0, `ac68faa20`, compiler date 2026-05-25 |
| Cargo | 1.96.0; frozen offline local dependency closure |
| PowerShell verifier | PowerShell 7.6.9 on Linux |
| Successful bootstrap profile | Release, with only `topaz_seed_compiler` changed to `opt-level=0` |
| Product dependencies | Public core/runtime plus the Topaz CLI and thin native boundary |

The full 13,077,120-byte `compiler/generated/native/compiler.rs` was read and
its SHA-256 matched the canonical manifest:

```text
5f32cee07a1adea1c67ba8ac8fbd4f3ee4051f019a7856fcdb228740a1d4b8b1
```

This is an actual byte check. It does not regenerate that file from Topaz or
prove the independent trustworthiness of the initial generated core.

## Build and native execution

1. Baseline Cargo checking completed successfully in approximately 2m29s.
2. The default optimized core build was deliberately stopped as memory use
   approached the 8 GiB limit. There was no compiler diagnostic establishing
   failure, and the process was not reported as OOM-killed.
3. Bootstrap then completed with the package-specific Cargo option
   `--config 'profile.release.package.topaz_seed_compiler.opt-level=0'`.
   The observed generator/core build took about 3m12s and the product build
   about 9.14s. These are observations from this machine and its cache state,
   not clean-build performance promises.
4. The canonical core compiled the CLI with `status: completed`,
   `kind: rust-source`, and empty resolver/checker diagnostic arrays. After
   the build-log changes, generation and product linking passed again; the
   cached product build took about 8.30s.
5. The actual POSIX smoke with `--native-build` passed. Core Kit checked and
   emitted source, created/read its request, verified inputs, built the native
   sample and ran it. The sample printed `Hello from Topaz Core Kit`. The full
   gate passed again after the final template and short-format changes.

The regenerated CLI Rust after adding the short diagnostic format argument
to the Topaz build policy was 209,353 bytes with SHA-256:

```text
4a76fa1d6bb533faaf32549e9f168eedef9badde069e7674d28ecfc596ac9768
```

This is a generated-source digest, not the digest of a release payload.
The follow-up bootstrap log was 1,257 bytes: generated-module attributes
suppressed 17 mechanical warnings, while short Cargo diagnostics retained
one generated-code parser warning without printing a huge source line.
Handwritten adapter diagnostics were not disabled globally.

The successful build used the unchanged canonical compiler bytes; it did not
use the website CLI, Rust reference compiler or private evaluator. Lowering
the core crate's Rust optimization level changes native product bytes and
performance, so this build cannot validate the default release configuration.

## Behavior and script checks

The additional CLI driver ran from a directory with Korean characters and
spaces. All 32 checks passed, including:

- Request admission failures for wrong identities/schema/producers, invalid
  budgets, duplicate facts/modules, missing entry/containment, unsupported
  mounts/fact kinds, private extern/replay and conflicting options.
- Unicode path source/request checks and a complete two-module closed request.
- Missing imported source fact rejection despite a corresponding file being
  present in the process working directory: no ambient-source fallback.
- Existing output preservation after rejected emission, refusal to overwrite
  an unmanaged build directory, and rejection of symlink output entries.

The POSIX byte verifier and the PowerShell semantic verifier both passed.
Thirty-two isolated verifier regression runs produced their expected results:
29 rejections and three positive/control results. These included changed
source bytes, malformed/duplicate inventory records and rehashed JSON with
incorrect source mappings or provenance. A byte-only verifier accepting a
coherently rehashed control does not establish JSON semantic equivalence;
PowerShell supplies those additional relationship checks.

Sixteen orchestration checks used controlled substitute tools to exercise
ordering, explicit optional regeneration, and error propagation without
repeating the expensive compiler build. The smoke negatives now require the
expected CLI failure status and diagnostic; an arbitrary crash does not count
as successful rejection. These orchestration cases are separate from the
actual product/native sample execution above.

The additional development drivers were scratch-only test instruments. Python
used by those instruments is not an installation, bootstrap, use or repository
smoke dependency. The checked-in smoke and verification paths use the stated
OS/Rust baseline.

## Reproducibility investigation

A small Rust probe included a generated module from two different checkout
paths. Without source-path remapping, the resulting executables had different
hashes and embedded their physical checkout paths. With stable
`--remap-path-prefix` mappings, both probe executables had the same hash and
used the fixed logical path in diagnostics.

This identifies a concrete requirement for the future release recipe. The
probe was not the complete Core Kit. Stable path mapping, exact build inputs,
compiler/linker/SDK identity, and a whole-payload comparison remain required.
The [Rust source-path mapping documentation](https://doc.rust-lang.org/rustc/remap-source-paths.html)
explains the textual prefix rules and the separate linker-path limitations.

## Follow-up: independent complete executable comparison

The next experiment started from merge commit
`4ab14c127665dafed3e2c6c7987ca384cf394c3d`, with the path-normalization wrapper,
shared smoke fixtures and native-manifest relocation in this change. The
canonical compiler, public runtime and all Topaz CLI source bytes remained
unchanged.

Two source exports used different-length checkout paths, one containing Korean
characters and spaces. Each had its own initially empty `compiler/target` and
Cargo home. Both generated the CLI into different external output directories.
No build artifacts were copied from one source export into the other.

The recorded Linux toolchain was Rust/Cargo 1.96.0, the Ubuntu GCC 13.3.0 driver,
Rust's bundled LLD 22.1.2 and host glibc 2.39. A small direct linker observation
confirmed `cc`, `-fuse-ld=lld` and the bundled linker search path. The successful
pair used `CARGO_BUILD_JOBS=2` and these **core-only development overrides**:

```toml
[profile.release.package.topaz_seed_compiler]
opt-level = 0
codegen-units = 256
```

An earlier attempt in this experiment used only `opt-level=0` and one Cargo
job. It was killed at the 8 GiB limit: rustc reported SIGKILL and the cgroup
OOM/kill counters each increased to one. This is a different event from the
initial default-profile build that was stopped deliberately. Neither event
establishes a general memory requirement or a successful default release.

With the smaller code-generation units, the independent complete builds took
approximately 219.3 and 219.7 seconds. Source-path mapping alone was insufficient:
their first native executables still differed by 272 bytes, despite identical
CLI Rust and identical public compiler/runtime `.rlib` and `.rmeta` files.
Cargo's absolute package identity for the native member outside the workspace
caused the remaining difference. See [REPRODUCIBILITY.md](REPRODUCIBILITY.md)
for the pinned Cargo source and the manifest-only correction.

After moving the native manifest under `compiler/core-kit-native/`, the native
targets were rebuilt separately in those same two source exports. Each retained
only its own independently built public dependencies, whose bytes were checked
again and remained unchanged. Those final native rebuilds took approximately
18.1 and 18.6 seconds.

The complete resulting executables matched by direct `cmp` and SHA-256:

| Item | Both builds |
| --- | --- |
| Native executable bytes | 101,701,848 |
| Native executable SHA-256 | `185d7a7cd52a1fe6475b69188b4cd83d66a57de784abcc11559596f20d4a5a21` |
| Generated CLI Rust bytes | 209,353 |
| Generated CLI Rust SHA-256 | `4a76fa1d6bb533faaf32549e9f168eedef9badde069e7674d28ecfc596ac9768` |

Neither executable contained its physical checkout or external output path.
Both passed version/help, an actual source check and `kit verify` against their
own data checkout. Both also passed input verification after copying the binary
inside their own checkout and removing `TOPAZ_CORE_KIT_ROOT`, exercising
executable-relative data discovery. The final matching artifact from build A
passed the full `--native-build` smoke, including native sample execution.

The ordinary bootstrap was separately rerun successfully after the manifest
move, using its existing core-only `opt-level=0` cache and without source-path
mapping. This guards the normal source-build entry point as well as the
separate reproduction recipe.

This is a complete executable reproduction result for the recorded same-host
Linux development configuration. It is not a default optimized release,
cross-machine verification, Windows/macOS result, or a comparison of an
assembled release payload with all its data and notices. The default release
profile was not changed, and these development binaries were not added to Git.

## Follow-up: exact smoke output

The follow-up smoke scripts now compare captured hash/sample output against
shared LF fixtures, using `fc /b` on Windows and `cmp` on POSIX. They preserve
the exact complete-output contract without `findstr /X`. The actual Linux hash
and native sample match the 65-byte and 26-byte fixtures. Sixteen controlled
POSIX smoke cases passed: two exact-output positives and fourteen rejections
for prefixes, suffixes, duplicated lines, extra blank lines, missing LF, CRLF
and empty output. The controlled POSIX cases do not establish Windows execution.
The subsequent owner-reported PR #4 run above covers the new CMD assertions.

Seven controlled wrapper checks covered Unicode/spaces/equals in paths,
physical output-directory resolution, inherited compiler flags, separator
characters and generated-file symlinks. These wrapper checks used a substitute
bootstrap; the complete native execution evidence is recorded separately above.

## Owner-reported Windows bootstrap and canonical regeneration

On 2026-10-31, the owner supplied the local agent's completed work transcript.
It records these results from a detached Windows worktree initially checked out
at the PR #1 merge, `4c44896ffdd14e83061e0b2a42cedc7196560b7f`:

| Step | Reported result and scope |
| --- | --- |
| `bootstrap.cmd` | Pinned Rust/Cargo 1.96.0 offline build completed in 17m23s, followed by CLI Rust generation and native Core Kit linking. The command uses the default profile; no profile override was shown in the transcript. |
| `core-kit/smoke.cmd --native-build` | Passed after the hash and sample assertions were changed from `findstr /X` to substring checks in `7d9219674f2e02a27c0d3d9e26779f7fdc8e38ef` and `aaeb4cc79ca4823d38077a8db838e71349f2061d`. |
| `bootstrap.cmd --verify-core` | Completed; the agent reported that the regenerated canonical compiler matched the checked-in `compiler/generated/native/compiler.rs` byte for byte. |

This is owner-supplied execution evidence. The transcript includes the commands
and agent conclusions, but not the complete successful terminal outputs,
regenerated-file hashes, full toolchain/linker/SDK identity, or inherited build
configuration. The temporary verification worktrees were subsequently removed.
The byte-comparison result is therefore recorded as reported, not as a second
comparison performed on the Linux validation host. It establishes a reported
core self-regeneration match, not complete native executable or whole-payload
reproduction on Windows.

The same transcript records review and merge of PR #3 as
`ec296cbb978f3b9b577268604c2b713bbc8b8c04` on 2026-10-30 at 23:54:57 KST.
Its Windows review ran `git diff --check` and WSL `bash -n` on the changed POSIX
scripts. It did **not** run bootstrap or native smoke on the PR #3 worktree.
In particular, the relocated native Cargo manifest and the stronger Windows
`fc /b` assertions were outside that earlier Windows execution result. The successful
Linux independent-executable comparison above covers that manifest correction
under its own recorded development profile.

PR #3 did not change the canonical compiler or its regeneration inputs. There
is no need to repeat the expensive core comparison solely to validate the
manifest relocation or smoke assertions. The owner subsequently ran the
ordinary bootstrap and native smoke on PR #4 merge and supplied artifact
identities, as recorded at the top of this document.

## Explicitly not established

- Completion of the default optimized Linux release build.
- Independent inspection of the complete reported Windows outputs and build
  environment, an independent Windows executable reproduction, Windows
  PowerShell 5.1 execution, or macOS support. The owner-reported PR #4 result
  covers the PR #3 manifest relocation and exact-output CMD assertions. The
  later PR #8 result adds project and Kit-build execution and matching generated
  CLI Rust, while its two executable files fail the byte comparison.
- Windows execution of current maintenance, packaging or release installation.
  The supplied execution transcript ends at PR #8; the later bounded image
  report does not exercise the new installer or kit refresh.
- A locally repeated canonical core comparison on the Linux validation host.
  The owner-reported Windows regeneration match is recorded above.
- Reproduction of the default optimized release or an assembled complete
  platform payload, including its data/notices. The development executable
  pair above is a narrower demonstrated result.
- Approved public prebuilt binaries, platform support promises or a published
  optional-download distribution.

No executable was added to `dist/` by this validation. Continue with
[LOCAL-VALIDATION.md](LOCAL-VALIDATION.md) for another platform and
[BUILD.md](BUILD.md) for the separate release gates.
