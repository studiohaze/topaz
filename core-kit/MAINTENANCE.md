# Maintaining the CLI inputs

Core Kit keeps its CLI source mappings, initial compiler input and verification
inventories in Git. These generated records let the source bootstrap start
without a website executable or an additional generation runtime.

Use the Topaz maintenance command after changing the CLI source set or other
managed inputs.

```sh
./out/core-kit-bootstrap/topaz-core-kit kit refresh --check
./out/core-kit-bootstrap/topaz-core-kit kit refresh
```

```powershell
.\out\core-kit-bootstrap\topaz-core-kit.exe kit refresh --check
.\out\core-kit-bootstrap\topaz-core-kit.exe kit refresh
```

An existing downloaded or locally built Kit with this command works as well.
The root launcher selects the catalogue version and its local receipt without
revalidating the source tree on every command, so an installed Kit can refresh
and build edited sources. Full source-input admission belongs to installation
or an explicit `kit install --check`. A source-built Kit can always be invoked
by its reported output path.
The selected checkout comes from the ordinary executable discovery rules or
TOPAZ_CORE_KIT_ROOT. The caller's current directory does not select a different
repository to rewrite.

## Check and update

The check form is read-only. It returns zero when the records already describe
the current managed inputs and the CLI passes the canonical compiler's type
check. A stale record or invalid input returns one with a diagnostic. It does
not create an output directory or lock.

The update form prepares all new records in memory, checks protected inputs
against their existing recorded hashes, and type-checks the current CLI with
the unchanged canonical compiler. It then updates only these four files.

- core-kit/cli/REQUEST.json
- core-kit/cli/initial/request.json
- core-kit/payload-inputs.data
- core-kit/verify-core.data

No Cargo build or core self-regeneration runs. After a successful refresh,
use kit build to create a new native executable. Review and commit the source
changes and generated records together. The command does not stage Git files,
commit, publish a payload or replace a downloaded installation.

The generated JSON uses the public JSON writer's deterministic representation
and a final LF. Treat the initial request as a generated artifact. The readable
Topaz modules remain the authoritative CLI source.

## What is discovered

The command discovers regular .tpz files beneath core-kit/cli/src, including
nested source directories. It creates matching source mappings, source facts,
physical-containment facts and complete virtual directory listings. An added
or removed helper module therefore does not require hand-editing embedded
source strings or directory entries. The entry remains src/main.tpz.

The four canonical standard boundary declarations come from the compiler seed
input. Additional flat .tpz declarations under core-kit/cli/std become explicit
standard mappings. The command rejects a replacement for a canonical standard
name and does not admit std.lispex.

Discovery is deterministic and bounded. Names must be portable repository
paths without case aliases, reserved Windows names or symlinks. The source
limit is 1,024 files, 256 directories and 32 directory levels. The initial
request's own resource budgets still apply and are preserved.

The payload path list keeps its explicitly curated entries and refreshes these
managed trees.

- core-kit/cli/src
- core-kit/cli/std
- core-kit/cli/templates
- core-kit/native
- core-kit/fixtures
- core-kit/distribution
- examples/core-kit

It also retains all input-inventory paths. New dependencies outside those
managed trees still need an explicit addition to the curated payload path
list and the appropriate source or component review. Refresh is not a general
Cargo dependency resolver or an inventory of every Git file.

Distribution recipes, notices and component records are also recorded as
verification inputs. Refresh updates their declared bytes when a reviewed
source change adds or edits those files. It does not enable releaseReady,
change a recipe's policy or approve a platform. Candidate assembly still
admits the recipe contract and requires the complete mapped inputs.

## Download catalogue and source identity

`core-kit/downloads.data` is deliberately excluded from the payload input list.
It contains the archive digest, executable digest and actual repository input
digest produced by `kit payload-pack`. Including its own bytes in those source
records would create a cycle between the archive and catalogue identities.
Maintenance must preserve this exclusion and reject an input list that tries
to add the catalogue.

Installer and launcher sources are reproducible inputs. They are curated
entries because their root and `core-kit/` locations are outside the automatic
source-tree discovery rules. Add or remove these paths deliberately when the
installation boundary changes, then run refresh and verify the resulting
inventory before building release candidates.

The input identity is SHA-256 of the canonical JSON representation of sorted
repository file records without a final LF. It binds their actual bytes rather
than the commit label declared by the publisher. A later catalogue-only commit
can select the same release inputs. Editing any listed installer, CLI, recipe
or other input changes the identity and needs a new matching release record.

Refresh does not generate a catalogue row, create an archive or authorize a
release. Leave the catalogue header-only until reviewed assets exist. Use
`kit payload-pack` for real release output and [PAYLOADS.md](PAYLOADS.md) for the
separate publication procedure.

## Protected inputs and preserved contracts

Canonical source pairs, the original public snapshot and immutable compiler
or seed inputs must still match their recorded hashes. Refresh refuses a
changed protected input and does not generate a replacement hash for it.
A coordinated compiler update needs its own source, seed and provenance work.

The descriptor and initial request must retain their recognized schemas,
compiler identity, entry and current six-argument callable ABI. Unknown fields
or inconsistent old mappings are errors. The command preserves resource
budgets, callable metadata and the configured generated Rust location.
It replaces source contents and facts with current files rather than silently
changing the surrounding compiler contract.

A temporary source module also checks the actual exported run function against
the complete native callback and return types. An extra parameter or changed
return contract therefore fails before a new snapshot is recorded. The module
and its small compilation allowance exist only for this check. They are not
saved in the request and do not change its recorded resource budgets.

This is consistency checking against the selected checkout. An inventory is
not independent publisher authentication. The command does not establish that
a native executable was built from the recorded inputs.

## Backups, locking and failures

Changed metadata is backed up under out/core-kit-refresh/run-N before any
repository file is replaced. Each run retains the previous and proposed file
bytes and a refresh.json receipt identifying their paths and SHA-256 values.
The recorded-input inventory is replaced last.

An exclusively created directory at out/core-kit-refresh/lock prevents two
refresh operations from claiming the same maintenance lock. The Topaz command
owns lock selection and release. Rust only supplies exclusive directory
creation, empty-directory removal and individual file-copy operations.

If replacement or final verification fails, the command attempts to restore
its own replaced metadata. It does not overwrite a file that has changed to
unexpected bytes during the operation. The diagnostic and receipt identify
whether rollback completed or recovery is required, and backups are retained.

The group of four files is not a crash-atomic filesystem transaction. Keep
sources stable while refreshing. An interruption can leave a lock or a partial
metadata update. Confirm that the earlier process has ended, inspect its
receipt and compare the retained before and after hashes before restoring
metadata or removing a stale lock. Do not remove another running process's
lock. Ordinary platform file-copy and empty-directory commands are sufficient
for recovery; no extra scripting runtime is needed.

A successful refresh followed by a second refresh is a no-op. It does not
create another backup run when none of the four metadata files changes.
