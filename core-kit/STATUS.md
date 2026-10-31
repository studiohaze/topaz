# Core Kit implementation status

## Source checkout and optional release installation

The release-download transition builds on PR #10 and the owner's diagnostic
commit `408313aea0240f595089db616888229e542c6bf7`. Git contains the Topaz CLI,
canonical public compiler, generated compiler Rust, seed inputs, public runtime
and source build materials. Platform executables belong in named GitHub
Releases and the checkout's ignored installation cache.

The public catalogue currently contains its schema header only. No release
asset, approved platform recipe or native binary has been published by this
change. An installation request without a matching target fails before a
network request and points to the independent source bootstrap.

The READMEs describe the completed product and its intended `studiohaze/topaz`
home. This page and [VALIDATION.md](VALIDATION.md) record actual availability
and execution evidence. No repository migration is performed by the product.

## Implemented

- Twenty-seven Topaz modules own commands, project configuration, compiler
  requests, diagnostics, native build policy, source verification, maintenance,
  image inspection, candidate preparation, packaging and installation policy.
- The public core and its callable ABI are unchanged. Compiler source queries
  go through the Topaz provider while compilation retains its no-capability
  host. No reference compiler, private evaluator or website CLI is used.
- One-package projects resolve local modules and the supported Topaz 5.21
  standard dependency. Saved requests replay complete source inputs. Native
  project builds preserve separate filesystem read and write grants.
- `kit build` recompiles the local Topaz CLI and links it with the canonical
  core. `kit refresh` discovers module changes, type-checks the CLI and updates
  its descriptor, snapshot and inventories together with retained backups.
- Source bootstrap starts with the pinned Rust/Cargo environment and local
  inputs. Ordinary bootstrap does not regenerate the compiler core. Frozen,
  offline Cargo resolution and explicit host-tool admission remain unchanged.
- Rust provides the compiler bridge and mechanical process and filesystem
  operations. The additional installation primitive renames a complete sibling
  directory after checking that its destination does not exist. Topaz owns the
  per-target lock, stage, validation, receipt and activation sequence.
- The CLI descriptor maps 27 modules and its initial request contains 58 facts.
  The byte verifier covers 50 canonical source pairs, 62 original snapshot files
  and 57 additional inputs. The payload input list contains 314 paths.
- `kit binary-info` reports bounded ELF64, PE32+ and thin Mach-O metadata and
  complete-byte comparison with at most sixteen difference samples. It does
  not execute the inspected image or establish a minimum supported OS.
- Candidate assembly binds the current compiler request, generated CLI Rust,
  executable, recipe, notices, component records and file modes. Recipes use
  schema v2 and a 256 MiB file bound. Complete release tar content is separately
  bounded to 256 MiB. The former 100 MiB ordinary Git gate is removed.
- Candidate comparison checks complete files, manifests and modes. Promotion
  requires native checks and an approved recipe and writes only to the explicit
  new output directory. It no longer selects a destination under `dist`.
- `kit payload-pack` checks the approved candidate pair, creates deterministic
  flat ustar with the public stored-DEFLATE gzip codec, and writes the archive,
  one catalogue row and `package.json` to a new output directory. It performs
  no network, Git or publishing operation. Equal candidates do not by
  themselves establish independent build history.
- Download packaging additionally inspects the executable's startup imports.
  Each import must be classified as a host-provided system dependency because
  the first installer stage starts that executable before extracting companions.
  Assembly and promotion can retain bundled runtime files; a download cannot
  rely on one of those files for its initial startup.
- `install-core-kit` and `install-core-kit.cmd` use a complete, strict catalogue
  with fixed target, repository, named tag, asset name, lengths and hashes.
  They verify the received archive and the extracted executable before its
  first run, then pass the archive to the Topaz installation command.
- `kit install --archive` verifies the full canonical container and current
  source inputs before materialization. It validates the staged native payload
  and creates a completion receipt before a single sibling rename. Existing
  versions remain available. Failed stages are retained and the owned lock
  is released on an ordinary error.
- `kit install --check` checks the receipt, executable, complete payload and
  current source inputs. It reconstructs the canonical archive from installed
  members and compares its complete identity with the catalogue, so rewriting
  a mutable payload manifest cannot substitute different companion bytes.
- The archive hash determines the local version directory. Root launchers
  select that version and exact receipt without network access, updates,
  compilation or repeated source-tree hashing. Edited sources can still be
  maintained and rebuilt through an installed Kit.
- The catalogue is deliberately excluded from payload and verifier input
  hashes. Its parser, installer and launcher code remain included. This avoids
  making the archive depend on its own published hash. Catalogue parsers and
  refresh admission enforce that boundary.
- No Python, Node, jq or GitHub CLI is required. POSIX first-stage installation
  uses ordinary shell utilities, curl, tar and a SHA-256 utility. Windows uses
  permitted local PowerShell helpers, .NET and tar.exe. Execution policy is
  respected. Source `bootstrap.cmd` remains available without those scripts.

The Linux and Windows x86_64 development recipes remain `releaseReady` false
and include their thirteen required notice/component companions. The Windows
template records exactly the ten imports in the supplied PR #8 comparison,
without claiming a minimum Windows version or bundling those system DLLs.
Package and installation
regression fixtures may explicitly approve that recipe inside a private test
checkout; this does not approve or publish the real platform release.

## Validation scope

[VALIDATION.md](VALIDATION.md) identifies each executable, generated source,
test run and remaining gate. Linux development builds retain the recorded
core-only opt-level=0 override and existing public dependency cache. They do
not establish the default optimized release or Windows/macOS execution.

PR #11 passed seventeen native adapter tests, including the three sibling-directory
publication tests covering existing destination preservation, non-directory
and symlink rejection. Independent installer checks cover strict catalogue parity,
bounded first-stage extraction, hash rejection, receipt selection and native
argument forwarding. PowerShell checks on Linux are not Windows PowerShell
5.1 execution evidence.

The PR #11 Linux executable passed the full repository smoke with both native
application builds, plus 108 packaging and installation commands and 358
assertions. Eighteen commands succeed and ninety exercise expected refusal.
Repeated packaging is byte-identical, a fresh offline wrapper installation
works, and archive, installed-file, lock and staging failures preserve existing
versions. These are private-fixture results from one supplied development
build, not an independently reproduced or published release.

The follow-up from PR #11 rebuilds the Topaz CLI and passes ordinary smoke.
Its strict-encoding matrix passes all 27 expectations across PowerShell,
POSIX and native Topaz verification. A focused 16-command regression passes
88 assertions for download startup admission, preserved runtime promotion,
older installation rejection and successful installation with an unused
companion. The canonical core and native adapter are unchanged. This follow-up
does not repeat the earlier native application build or adapter unit tests.

Earlier PR #10 evidence includes full native smoke, 72 image cases, 29 complete
candidate commands and 149 assertions. Its path-selection comparison linked
the same CLI from two output paths within one checkout and cache and obtained
identical complete Linux executable bytes. These are historical artifact
identities and are not substituted for transition validation.

## Reported Windows evidence

The owner successfully ran PR #8 bootstrap, expanded native smoke and
`kit build` at `b6a97245189365f93801f9e996c3c3152bcd3e3e`. The two generated CLI
Rust files were byte-identical, but the 131,391,488-byte executables differed.
The worktree, Cargo cache, logs and both original executables are retained.

After PR #10, the owner published the bounded
[native comparison](validation-pr8-native-comparison.json). The PE targets,
section extents and imported DLL lists match. Timestamps and PDB GUIDs differ,
and all five recorded section hashes differ, including `.text`. The cause is
not established by those hashes. Removing a timestamp alone cannot establish
whole-executable identity for this pair.

At PR #11 merge `e322fa2`, the [two Windows EXEs](validation-pr11-native-comparison.json)
had identical code sections and imports. Their `.rdata`, PE timestamps and
PDB GUIDs differed. Native smoke passed with the bootstrap Kit. The rebuilt
Kit passed its version, project and input checks. Payload comparison rejected
the resulting inventories.

At PR #13, Windows bootstrap and `kit build` produced identical 145,224,192-byte
executables in the same checkout. Native smoke and complete payload comparison
passed. The owner retained both outputs and the comparison report.
[PR #13 native Windows result](VALIDATION.md#pr-13-native-windows-result)

The owner used that pair to pack and install a pinned archive offline on a
Windows 11 x64 development machine. The installed launcher passed its version,
project and installed-byte checks. [PR #14 installation record](VALIDATION.md#pr-14-windows-offline-installation)

At PR #15 merge `2d3af629`, bootstrap and `kit build` produced identical
145,312,256-byte Windows EXEs. The native packer accepted the matching
linked CLI digest. The private archive passed offline installation, project
checking and `kit install --check` on the development host.
[PR #15 validation and receipts](VALIDATION.md#pr-15-windows-build-and-offline-installation)

A fresh copy of the matching source fixture also installed that archive on
the same Windows 11 host with Rust, Cargo and Visual Studio excluded from
the process PATH. [Installation receipt and host details](validation-pr16-no-build-tools-install.json)

## Remaining release work

- First installation on a Windows host without Rust or Visual Studio, with the
  available Visual C++ runtime recorded. The private packaging fixture already
  exercised a Windows maintenance write when it refreshed the changed recipe.
- Default-profile platform measurements, actual system/runtime observations,
  minimum OS evidence and approved platform recipes. macOS needs native
  validation as well.
- Complete independent reproduction of each final platform payload and
  archive. Deterministic packaging of one supplied build is a separate result.
- Review of the final platform notices, then publication of the
  actual named release assets and their measured catalogue rows.
- Preparation of the intended default branch and repository home.

Additional package dependencies and broader project configuration remain a
separate compiler/provider contract review. The current project contract is
documented in [the command guide](cli/README.md).

[LOCAL-VALIDATION.md](LOCAL-VALIDATION.md) provides the remaining native handoff.
The source bootstrap remains usable while no matching public asset exists.
