# Distribution recipe inputs

This directory contains the reviewed inputs used to assemble a Core Kit
payload. Assembly policy lives in Topaz. Recipes select exact repository
files, target identities and system dependencies without downloading files
or running another packaging runtime.

## Development and release recipes

Select a recipe for the target and build profile you are validating. Each
recipe records the platform requirements, observed system dependencies and
companion files for that candidate. The target name alone does not describe
an available payload or establish compatibility with a particular OS version.

Development recipes support assembly and comparison while keeping promotion
disabled. The Linux x86-64 development recipe is
linux-x86_64-development.json. It preserves the existing notices, component
records and the pinned Rust standard-library notice.

The Windows x86-64 development recipe is windows-x86_64-development.json.
It classifies the ten DLL imports observed in both PR #8 executables and
recorded in [the native comparison](../validation-pr8-native-comparison.json).
It preserves the same companion files without bundling Windows runtime DLLs.
Those import observations establish neither current-revision execution nor a
minimum supported Windows version. Both development recipes keep releaseReady
set to false.

A recipe with releaseReady set to true permits native promotion and release
packaging after the
complete candidate comparison and execution checks pass. That setting is
reviewed repository policy. It accompanies the selected build profile, native
platform evidence and component review, and does not independently
authenticate their origin.

Download archives additionally require the executable's startup imports to
be satisfied by the declared system dependencies. A library supplied only as
a runtime companion cannot satisfy this first-run condition because the
installer starts the executable before unpacking companions. Assembly and
explicit promotion can still carry such a runtime file. An extra companion
that is not a startup import does not make a download ineligible.

## Recipe contract

Recipes use the topaz.core-kit-release-recipe/v2 schema and canonical JSON
with sorted object keys, no insignificant whitespace and one final LF.
File mappings are sorted by their flat destination names. System dependencies
are sorted and unique. Every mapped source and the recipe itself must appear
in core-kit/payload-inputs.data.

Notice and component mappings preserve the required existing texts and
descriptive records. Additional material belongs under this distribution
directory. Runtime mappings use runtime/<Rust-target>/ so the recipe cannot
silently collect arbitrary host files. Symlinks, path traversal, duplicate
names, case aliases and reserved assembly or executable names are rejected.

The release file limit is fixed at 268,435,456 bytes per file. The complete
uncompressed archive also fits within 256 MiB, including its headers and
padding. Assembly and packaging reject oversized inputs. Received archive
bytes are limited separately to 512 MiB. These are payload-format limits,
independent of Git blob storage. Packaging does not split, rebuild or silently
replace the executable that passed comparison.

PE system dependency names use lowercase ASCII. ELF names retain their exact
loader identity. Mach-O system dependencies must be canonical absolute install
names under /usr/lib/ or /System/Library/. An unresolved @rpath dependency is
not treated as a system library merely because its basename looks familiar.

## Release catalogue

The recipe and its mapped inputs belong to the reproducible source inventory.
`core-kit/downloads.data` is separate. It records the named GitHub Release
asset, archive and executable identities, and the canonical repository input
identity. The catalogue itself is excluded from the input inventory so adding
an archive hash does not change the archive's own recorded inputs.

`kit payload-pack` writes a catalogue row with the deterministic archive.
Publishing that asset and reviewing the row are separate operations. The
source commit declared by the publisher is provenance. Exact input equality
is checked through the recorded input identity, including the installer
sources. A later commit that changes only the catalogue can select the same
source inputs without rebuilding the payload.

## Notice provenance

COMPONENTS.json describes the native Cargo closure, its license
authorizations and the existing source policy. NOTICE-SCOPE.md states the
Apache-2.0 license of the Core Kit files and explains the inherited website
inventory and the distinction from Core Kit's selected components.

RUST-STDLIB-NOTICES.html is copied without modification from the pinned Rust
1.96.0 release. RUST-STDLIB-PROVENANCE.json records its exact archive member,
byte count and SHA-256. The archive hash was compared with the downloaded
Rust channel manifest, and the copied bytes matched both the installed and
extracted notice.

Keep volatile logs, timestamps and absolute build paths in external validation
records. They do not belong in the deterministic payload that two builds
compare. The repository's status and validation documents record which
recipes and operations have actually been exercised.
