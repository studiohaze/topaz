# Core Kit downloads

[Build Core Kit from source](../README.md#build-the-kit-from-source) for this
checkout. Its [catalogue](../core-kit/downloads.data) has no platform archives
listed. The source build puts the executable under `out/core-kit-bootstrap`.

## Install the Kit selected by your checkout

When the catalogue lists a GitHub Release asset for your platform, the
following commands install and run it.

```sh
./install-core-kit
./topaz --version
```

```powershell
.\install-core-kit.cmd
.\topaz.cmd --version
```

The explicit installer reads `core-kit/downloads.data`, detects the native
target and selects its one pinned release asset. A target without a catalogue
entry fails before making a network request and points to the
[source build](../core-kit/BUILD.md). It does not select a moving latest
release or download the website's full CLI.

On Linux and macOS, installation uses `curl`, `tar` and either `sha256sum`
or `shasum`, plus ordinary shell tools. Windows uses Windows PowerShell 5.1
or later, its .NET runtime and `tar.exe`. The checked-in helpers must be
permitted by the host's script policy. The entry points honor that policy
without changing it. Python, Node, jq and the GitHub CLI are not required.

You can supply an already downloaded archive instead of requesting it from
GitHub.

```sh
./install-core-kit --archive /path/to/topaz-core-kit-x86_64-unknown-linux-gnu.tar.gz
```

```powershell
.\install-core-kit.cmd --archive C:\Downloads\topaz-core-kit-x86_64-pc-windows-msvc.tar.gz
```

Both forms use the same catalogue and full validation. Supplying a file does
not authorize a different version, target or source input set.

## Target names and system requirements

The catalogue uses the following native target names.

| Target | Host family | CPU | Executable |
| --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Linux | x86-64 | `topaz-core-kit` |
| `aarch64-unknown-linux-gnu` | Linux | ARM64 | `topaz-core-kit` |
| `x86_64-pc-windows-msvc` | Windows | x86-64 | `topaz-core-kit.exe` |
| `aarch64-pc-windows-msvc` | Windows | ARM64 | `topaz-core-kit.exe` |
| `x86_64-apple-darwin` | macOS | x86-64 | `topaz-core-kit` |
| `aarch64-apple-darwin` | macOS | Apple silicon | `topaz-core-kit` |

A catalogue row determines availability for a checkout. This naming table
does not promise that every target has a published asset. The selected
payload's `assembly.json`, repository recipe and native validation records
define its platform requirements. An OS family and CPU name alone do not
establish a minimum OS version or runtime ABI.

The release executable starts using only the platform libraries declared by
that release. Payload companions are installed after this first run. A Windows
release records any required Visual C++ runtime among its platform
requirements. Before publication, validate its first run on a machine without
Rust or Visual Studio and record any separately installed runtime.

## Update or switch a checkout

The catalogue in the selected Git revision chooses the local Kit. After
updating or switching revisions, run the explicit installer if that catalogue
selects an archive you have not installed. The normal launcher reports the
missing version and keeps earlier installations intact.

```sh
./install-core-kit
./topaz --version
```

```powershell
.\install-core-kit.cmd
.\topaz.cmd --version
```

Returning to an earlier matching revision selects its retained installation.
Keep that revision's sources and catalogue together. Source-build outputs
remain separate under their chosen directories. Use `--archive` for an offline
installation of the newly selected version and `kit install --check` to check
its full payload and current source inputs.

## What is checked before use

The OS adapter checks the downloaded archive's exact byte count and SHA-256
against the catalogue before extracting the fixed executable member into a
private temporary file. It verifies that executable's byte count and SHA-256
before the first execution.

The candidate then runs the Topaz `kit install` command. Topaz validates the
complete archive, assembled payload, native target and current checkout input
identity. It checks required companions and notices, stages a new version and
activates it only after validation. Previously installed versions are retained.

These checks bind the downloaded bytes to the reviewed catalogue and checkout.
Keep the catalogue and source change under the same review as the release
records. Hashes identify files and do not independently authenticate a
publisher or establish a build's history.

## Installed layout

An installation uses a version directory under
`out/core-kit-installed/<target>/<archive-sha256>/`. Its `payload/` directory
holds the executable and complete companion set. The sibling
`installation.data` receipt records the target, archive and repository input
identity.

| File or group | Purpose |
| --- | --- |
| `topaz-core-kit` or `topaz-core-kit.exe` | The native Kit selected by the root launcher |
| `generated_cli.rs` | CLI Rust emitted by the canonical compiler for the candidate |
| `assembly.json` | Recipe and compiler input identities, native profile, platform requirements and file roles |
| `payload.json` | Exact payload membership, hashes and declared repository inputs |
| Component records and notices | Selected component inventory, provenance and existing terms |
| Platform runtime files | Companions required by the target recipe |

The `COMPONENTS.json` and `NOTICE-SCOPE.md` records retain their stated scope.
Copying their texts does not assign a new license to other repository files.
POSIX executables retain mode `0755` and companions mode `0644`. Windows
metadata does not claim Unix permission bits.

The root `topaz` and `topaz.cmd` launchers select the exact installed version
named by the current catalogue, preserve the caller's working directory and
arguments, and return the executable's exit status. They do not download,
update or compile anything. They select the local version and receipt without
rechecking every source input, so the installed Kit can build edited sources.
Use `kit install --check` for full explicit installed-payload and current-input
verification. That explicit check reconstructs the canonical archive from
installed files and matches its digest against the catalogue as well as
checking the file records. Repeating installation verifies an existing
matching version rather than replacing its files.

Keep the complete checkout when moving an installation. The Kit uses its
source inputs, standard modules and native build templates. A source bootstrap
writes a separate executable under `out/core-kit-bootstrap` and does not
activate a catalogue version. Invoke that source-built executable by its
reported path.

## Prepare a release

Use Topaz's assembly, comparison and packaging commands with independently
recorded builds and an approved target recipe. Promotion writes a new staging
directory selected with `--out-dir`. Packaging writes a deterministic archive
and a catalogue row into a separate output directory.

```text
topaz-core-kit kit payload-assemble --build-dir <build> --recipe <repository-relative-recipe> --out-dir <new-candidate>
topaz-core-kit kit payload-compare --payload-dir <first-candidate> --other <second-candidate>
topaz-core-kit kit payload-promote --payload-dir <first-candidate> --other <second-candidate> --out-dir <new-stage>
topaz-core-kit kit payload-pack --payload-dir <first-candidate> --other <second-candidate> --out-dir <new-release> --repository studiohaze/topaz --tag <release-tag> --source-commit <40-character-commit>
```

The publisher uploads the resulting archive to the named GitHub Release and
reviews the generated catalogue row for the source checkout. These commands
do not upload, create a release or modify Git. Executable bytes are not
committed under this directory.

The [payload workflow](../core-kit/PAYLOADS.md) defines the publication and
installation checks. The [reproduction guide](../core-kit/REPRODUCIBILITY.md)
explains complete comparisons and independent build evidence.
