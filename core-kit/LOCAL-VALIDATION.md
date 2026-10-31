# Local validation

Use this guide to check a changed Kit on a native host and return a useful
result without repeating unrelated compiler reconstruction.

The owner-reported Windows run at PR #8 merge
`b6a97245189365f93801f9e996c3c3152bcd3e3e` passed ordinary bootstrap, full native
smoke and actual Topaz `kit build`. It covered project checks and builds,
request replay, payload verification and native sample/project execution.
The newly built Kit also passed version, project check and `kit verify`.

The two generated CLI Rust files matched completely, but the two Windows
executables differed despite both being 131,391,488 bytes. The owner has now
completed their bounded `kit binary-info` comparison and supplied
[the original report](validation-pr8-native-comparison.json). That inspection
is complete and does not need repeating for this transition.

The later [PR #11 comparison](validation-pr11-native-comparison.json) at
`e322fa2` narrows the difference. Bootstrap and `kit build` produced identical
CLI Rust and matching code sections. The two final EXEs differ in `.rdata`,
PE timestamps and PDB GUIDs. Both payloads assembled and their inventories
failed comparison. Keep the retained PR #11 worktree with its two outputs,
`out\pr11-native-comparison.json` and `out\core-kit smoke-29611-379`.

## Preserve the completed PR #8 evidence

Keep the retained PR #8 worktree at its reported revision with
`compiler/target` and both earlier output directories intact. Its smoke logs
are under `out\core-kit smoke-17111-2065`. Keep the supplied comparison report
with those artifacts. Do not overwrite either old executable while building
the current CLI.

Use the updated development checkout and its existing Cargo cache for new
work. A clean Git worktree can retain ignored Cargo artifacts. A new empty
build cache is unnecessary for the current CLI and installer checks. If logs
are copied to an archive, confirm that the copy succeeded before removing
the originals. The PR #4 report retained its logs in the original worktree
after archive-copy attempts failed.

## What the completed comparison shows

The supplied report identifies the same two file hashes recorded for the
PR #8 pair and reports `sameBytes` false. Both files have the same length,
imported DLL list, section boundaries and section lengths.

| Observation | Bootstrap executable | Kit-build executable |
| --- | --- | --- |
| Complete bytes | 131,391,488 | 131,391,488 |
| PE timestamp | `1793396347` | `1793396526` |
| PDB GUID | `2a0964b8-d105-4cff-8f38-db1bad1a6269` | `76ae548f-a2cf-4f92-83ec-ea753879f630` |
| PDB path | `topaz_core_kit.pdb` | `topaz_core_kit.pdb` |

Every reported section hash differs, including `.text`, `.rdata`, `.data`,
`.pdata` and `.reloc`. The observed differences therefore extend beyond the
PE timestamp and PDB identity fields. Removing only those metadata differences
would not make the files equal. The report does not establish the responsible
compiler or linker decision, source-path effect or other cause.

PR #10 later staged the selected CLI Rust at `OUT_DIR/generated_cli.rs`.
PR #13 added the Windows release linker settings. The PR #13 and PR #15
native comparisons both found identical EXEs from the two build paths in a
shared checkout. [Windows evidence](STATUS.md#reported-windows-evidence)

## Windows full regression gate

PR #15 passed the Windows build, payload comparison and development-host
offline installation checks. The retained archive is ready for the
[first installation on a clean host](#first-run-on-an-ordinary-windows-host).

Run this in a development checkout at the revision containing the link change.
Confirm the full SHA against the revision under test before starting. Keep the
PR #8 and PR #11 worktrees intact. The existing development Cargo cache can
be reused. Ordinary bootstrap suffices for this check.

```powershell
$env:RUSTUP_TOOLCHAIN = '1.96.0'
$env:RUSTUP_AUTO_INSTALL = '0'
git rev-parse HEAD
git status --short
rustc -vV
cargo -vV
```

Use a PowerShell session opened for this run. Record the linker and SDK
versions, inherited Cargo settings, flags and any profile overrides. The
default x64 MSVC release should pass `/INCREMENTAL:NO`, `/Brepro` and
`/DEBUG:NONE` to the final Kit link. A release with debug information enabled
keeps the normal PDB behavior.

Give each run its own output directory so both retained pairs and past smoke
logs stay available. The bootstrap and Topaz commands reuse the development
checkout's Cargo cache. The explicit smoke argument selects this run's EXE.

```powershell
$env:CARGO_TERM_VERBOSE = 'true'
$env:TOPAZ_CORE_KIT_ROOT = (Resolve-Path .).Path
$runDir = "out/relink-$(Get-Date -Format yyyyMMdd-HHmmss)"
if (Test-Path -LiteralPath $runDir) { throw 'Choose a fresh output directory.' }
New-Item -ItemType Directory -Path $runDir -ErrorAction Stop | Out-Null
$bootstrapDir = "$runDir/bootstrap"
$buildDir = "$runDir/kit-build"
$bootstrapKit = "$bootstrapDir/topaz-core-kit.exe"
$rebuiltKit = "$buildDir/topaz-core-kit.exe"

.\bootstrap.cmd --out $bootstrapDir
if ($LASTEXITCODE -ne 0) { throw 'Retain the bootstrap error.' }
.\core-kit\smoke.cmd $bootstrapKit --native-build
if ($LASTEXITCODE -ne 0) { throw 'Retain the smoke logs.' }
& $bootstrapKit kit build --out-dir $buildDir
if ($LASTEXITCODE -ne 0) { throw 'Retain the Kit build outputs.' }
& $rebuiltKit --version
if ($LASTEXITCODE -ne 0) { throw 'The rebuilt Kit did not start.' }
& $rebuiltKit check --root examples/core-kit/project
if ($LASTEXITCODE -ne 0) { throw 'The rebuilt Kit failed the project check.' }
& $rebuiltKit kit verify
if ($LASTEXITCODE -ne 0) { throw 'The rebuilt Kit failed input verification.' }
```

Keep the bootstrap and Kit build output with the smoke logs under
`out/core-kit smoke-.../`. Compare the generated Rust first, then capture
the complete EXE hashes and the bounded image report before enforcing equality.

```powershell
$leftRust = "$bootstrapDir/generated_cli.rs"
$rightRust = "$buildDir/generated_cli.rs"
Get-FileHash -Algorithm SHA256 -LiteralPath $leftRust,$rightRust,$bootstrapKit,$rebuiltKit
fc.exe /b $leftRust $rightRust > $null
if ($LASTEXITCODE -ne 0) { throw 'The two generated CLI Rust files differ.' }

$report = & $bootstrapKit kit binary-info --file $bootstrapKit --other $rebuiltKit
if ($LASTEXITCODE -ne 0) { throw 'Retain the native comparison error.' }
$report | Set-Content -LiteralPath "$runDir/native-comparison.json" -Encoding UTF8
$comparison = ($report -join "`n") | ConvertFrom-Json
$comparison | Select-Object sameBytes, firstDifferentOffsets
if ($comparison.sameBytes -ne $true) {
    throw "The final EXEs differ. Retain both builds and $runDir/native-comparison.json."
}
```

After a byte match, assemble the two builds separately and compare their
complete payloads using the checked-in development recipe.

```powershell
$recipe = 'core-kit/distribution/windows-x86_64-development.json'
& $bootstrapKit kit payload-assemble --build-dir $bootstrapDir --recipe $recipe --out-dir "$runDir/payload-a"
if ($LASTEXITCODE -ne 0) { throw 'Bootstrap payload assembly failed.' }
& $bootstrapKit kit payload-assemble --build-dir $buildDir --recipe $recipe --out-dir "$runDir/payload-b"
if ($LASTEXITCODE -ne 0) { throw 'Kit-build payload assembly failed.' }
& $bootstrapKit kit payload-compare --payload-dir "$runDir/payload-a" --other "$runDir/payload-b"
if ($LASTEXITCODE -ne 0) { throw 'The assembled payloads differ.' }
```

Return the comparison report and payload result with the source revision and
native environment. A matching pair from this checkout establishes agreement
between the two build paths with a shared cache. Independent builds and a
clean-host archive installation have their own release checks below.

A first native core build can be expensive even without self-regeneration.
Keep it running while it is compiling unless an actual error or resource
limit requires stopping it. Progress messages distinguish the core build,
CLI generation, native link and any explicitly requested core comparison.

## Linux and macOS

Use the same sequence with the POSIX entry points.

```sh
(
set -e
./bootstrap
./core-kit/smoke.sh --native-build
./out/core-kit-bootstrap/topaz-core-kit kit build
./out/core-kit-build/topaz-core-kit --version
./out/core-kit-build/topaz-core-kit check --root examples/core-kit/project
./out/core-kit-build/topaz-core-kit kit verify
cmp out/core-kit-bootstrap/generated_cli.rs out/core-kit-build/generated_cli.rs
)
```

The subshell stops at the first failure. Keep the same build
configuration throughout the comparison and record any nondefault profile.
Smoke logs remain under `out/core-kit smoke.XXXXXX/`. Either platform's smoke
script accepts a different Kit executable as its first argument.

## What the smoke gate covers

- Help and version return without waiting for stdin EOF.
- Complete hash output matches the checked-in digest and single LF fixture.
- `kit build-id` runs on the selected executable. When its generated CLI Rust
  is present beside it, smoke compares the reported digest with that file's SHA-256.
- `kit binary-info` parses the tested native executable and retains a nonempty
  `binary-info.json` report plus separate stderr. On a native Windows run this
  exercises the actual PE image and its native build metadata.
- The canonical core checks a valid source and rejects a type error.
- Source requests can be generated and read back without ambient fallback.
- A project manifest and local imports are handled through the source provider.
- Project request snapshots replay through the closed-input path.
- Project requests and Rust emission create missing output directories.
- A root without a valid `topaz.toml` returns the expected ordinary error.
- Rejected emission does not create a generated Rust output.
- The Topaz verifier checks the committed input inventory.
- The maintenance check type-checks the actual CLI callable contract and
  requires current source mappings, snapshot and inventories without writes.
- Payload commands record and verify the actual executable and reject an
  additional unrecorded file.
- With `--native-build`, both the source sample and saved project input build
  and run with exact expected output.

Both smoke scripts compare complete captured output against common LF
fixtures using POSIX `cmp` or Windows `fc /b`. Prefixes, suffixes and extra
lines fail. The checks do not depend on `findstr /X` or accept a matching
substring as complete success.

Smoke does not rebuild the Kit. The separate `kit build` step checks current
CLI input preparation, the canonical compile callback, toolchain metadata
capture, native linking and executable copying. None of these paths needs
Python, Node or jq, or a repeat of canonical compiler self-regeneration.

The retained payload candidate is an execution fixture and is not a complete
platform release with all notices and support records. A successful smoke run
also does not claim exhaustive operating-system capability validation.

## Separate maintenance follow-up

The completed PR #8 image report does not cover the maintenance command.
Use this separate Windows gate when an actual metadata update needs checking. Reuse the updated checkout's existing executable and Cargo cache.
Current smoke includes the read-only refresh check. The following additional
check exercises an actual metadata update in a separate worktree using that
executable. It does not build another compiler.

Run from the updated repository root with `$bootstrapKit` set to the executable
from the Windows gate above. The selected commit must contain the
maintenance implementation and its refreshed metadata. Keep the worktree and
its backup receipt for inspection.

```powershell
$kit = (Resolve-Path -LiteralPath $bootstrapKit -ErrorAction Stop).Path
$revision = git rev-parse HEAD
$fixture = Join-Path (Split-Path (Get-Location).Path -Parent) ('topaz-refresh-check-' + [Guid]::NewGuid().ToString('N'))
git worktree add --detach $fixture $revision
if ($LASTEXITCODE -ne 0) { throw 'Could not create the isolated maintenance worktree.' }

$previousRoot = $env:TOPAZ_CORE_KIT_ROOT
try {
    $env:TOPAZ_CORE_KIT_ROOT = $fixture
    & $kit kit refresh --check
    if ($LASTEXITCODE -ne 0) { throw 'Initial maintenance check failed.' }

    $source = Join-Path $fixture 'core-kit\cli\src\main.tpz'
    [IO.File]::AppendAllText($source, "`n// Native maintenance validation edit.`n", (New-Object Text.UTF8Encoding($false)))
    $previousErrorAction = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $stale = & $kit kit refresh --check 2>&1
        $staleExit = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousErrorAction
    }
    $stale | Out-Host
    if ($staleExit -ne 1 -or ($stale -join "`n") -notmatch 'need refresh') {
        throw 'The changed source was not reported as stale.'
    }

    & $kit kit refresh
    if ($LASTEXITCODE -ne 0) { throw 'Metadata refresh failed. Retain the backup receipt.' }
    & $kit kit refresh --check
    if ($LASTEXITCODE -ne 0) { throw 'The refreshed metadata is inconsistent.' }
    & $kit kit verify
    if ($LASTEXITCODE -ne 0) { throw 'The refreshed input hashes differ.' }
    & $kit kit refresh
    if ($LASTEXITCODE -ne 0) { throw 'The no-change refresh failed.' }
    git -C $fixture status --short
} finally {
    $env:TOPAZ_CORE_KIT_ROOT = $previousRoot
}
```

Expect the added source comment and generated metadata changes only. The
backup receipt under out/core-kit-refresh should say applied, and its lock
directory should be absent. The second refresh should report that the
snapshot and inventories already match. Return the exact revision, executable
hash, command output and receipt if a step fails. This mutation check still
needs Windows execution and is separate from the recorded Linux rollback test.

## Optional PowerShell verifier check

This is separate from the default Windows command path. In an environment
where PowerShell scripts are already allowed, parse the optional scripts and
inspect the parse errors before running the semantic verifier.

```powershell
foreach ($path in @('core-kit/bootstrap.ps1', 'core-kit/verify-core.ps1')) {
    $tokens = $null
    $errors = $null
    $resolved = (Resolve-Path -LiteralPath $path).Path
    [void][Management.Automation.Language.Parser]::ParseFile($resolved, [ref]$tokens, [ref]$errors)
    if ($errors.Count -ne 0) { $errors | Format-List; throw "Parse failure in $path" }
}
.\core-kit\verify-core.ps1
```

The PowerShell verifier adds JSON relationship checks to the common byte
inventory. Changing execution policy is not part of this handoff.

## What to return on failure

Return the exact commit, command, first error and relevant log. For CLI
compilation failures, preserve `cli.request.json` and `cli.response.json` in
the selected bootstrap or Kit-build output. For a smoke failure, retain the
stage named by the script and its whole log directory.

For project failures, include the manifest and the smallest source set that
reproduces the error. Saved project inputs are useful when filesystem facts
or relocation are involved. Keep existing expensive Cargo outputs so a fix
can be checked without rebuilding unchanged dependencies.

## Release installer validation

### Offline installation from the PR #13 pair

Use the two Windows build directories retained under
`out\relink-20261031-1151` in the PR #13 worktree.
The development candidates record the original recipe hash. The test below
assembles fresh candidates against a private recipe in a new detached worktree.
It reuses the original compiler requests, responses, generated Rust and EXEs.
The source commit in the test catalogue identifies the retained checkout.
The input-set digest identifies the modified fixture.

Run in Windows PowerShell on the machine with both outputs. The settings
at the start of the example identify the retained PR #13 pair. Replace the
bracketed values with the local worktree path and a private test repository
name. For a later
revision, complete the native comparison first and supply its checkout, run
directory, Git tree, measured EXE hash and private test tag. Keep the fixture local, with its
uncommitted recipe, refreshed inventory and test catalogue.

```powershell
& {
    $ErrorActionPreference = 'Stop'
    function Invoke-Checked($program) {
        & $program @args
        if ($LASTEXITCODE -ne 0) { throw "$program $args failed (exit $LASTEXITCODE)." }
    }

    $retained = '<retained PR #13 worktree>'
    $run = "$retained/out/relink-20261031-1151"
    $expectedTree = 'c962658dbd510f4d8c50a6e0ee0efd4826eb9dfe'
    $expectedExe = '2fd7f98d5faf00d13c331703bd37b0603216b15503e4113607a7946f9ee169c9'
    $testRepository = '<owner/name>'
    $testTag = 'pr13-offline-fixture'
    $kit = "$run/bootstrap/topaz-core-kit.exe"
    $revision = Invoke-Checked git -C $retained rev-parse HEAD
    $tree = Invoke-Checked git -C $retained rev-parse 'HEAD^{tree}'
    if ($tree -ne $expectedTree) { throw 'Checkout differs from the tested source tree.' }
    foreach ($exe in @($kit, "$run/kit-build/topaz-core-kit.exe")) {
        if ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ne $expectedExe) { throw "Unexpected EXE: $exe" }
    }

    $fixture = Join-Path (Split-Path $retained -Parent) ('topaz-offline-' + [guid]::NewGuid().ToString('N'))
    Invoke-Checked git -C $retained worktree add --detach $fixture $revision
    $savedRoot = $env:TOPAZ_CORE_KIT_ROOT
    Push-Location -LiteralPath $fixture
    try {
        $env:TOPAZ_CORE_KIT_ROOT = $fixture
        $recipe = 'core-kit/distribution/windows-x86_64-development.json'
        $path = (Resolve-Path -LiteralPath $recipe).Path
        $text = [IO.File]::ReadAllText($path)
        $before = '"releaseReady":false'
        if (-not $text.Contains($before)) { throw 'Expected the development recipe.' }
        [IO.File]::WriteAllText($path, $text.Replace($before, '"releaseReady":true'), [Text.UTF8Encoding]::new($false))
        Invoke-Checked $kit kit refresh
        Invoke-Checked $kit kit verify

        Invoke-Checked $kit kit payload-assemble --build-dir "$run/bootstrap" --recipe $recipe --out-dir out/candidate-a
        Invoke-Checked $kit kit payload-assemble --build-dir "$run/kit-build" --recipe $recipe --out-dir out/candidate-b
        Invoke-Checked $kit kit payload-compare --payload-dir out/candidate-a --other out/candidate-b
        Invoke-Checked $kit kit payload-pack --payload-dir out/candidate-a --other out/candidate-b `
            --out-dir out/private-package --repository $testRepository --tag $testTag --source-commit $revision
        Copy-Item -LiteralPath 'out/private-package/downloads.data' -Destination 'core-kit/downloads.data'
        $archive = (Resolve-Path 'out/private-package/topaz-core-kit-x86_64-pc-windows-msvc.tar.gz').Path
        Invoke-Checked .\install-core-kit.cmd --archive $archive
        Invoke-Checked .\topaz.cmd --version
        Invoke-Checked .\topaz.cmd check --root examples/core-kit/project
        Invoke-Checked .\topaz.cmd kit install --check
        Write-Output "Retain fixture $fixture and archive $archive"
    } finally {
        [Environment]::SetEnvironmentVariable('TOPAZ_CORE_KIT_ROOT', $savedRoot, 'Process')
        Pop-Location
    }
}
```

Record the archive hash and `out/private-package/package.json` with the
installer output and the selected version's `installation.data`. Record the
Windows version, architecture, PowerShell and `tar.exe` versions, and the host
execution policy. A successful run here covers offline installation on this
development machine. The public catalogue and platform recipe are maintained
in this repository.

### Windows negative control for the linked CLI

Reuse the PR #14 private checkout, whose test recipe and refreshed inventory
match the PR #13 source. Put its three genuine PR #13 compiler files beside
a copy of the PR #15 EXE. This controlled mismatch keeps the compiler records
consistent, so assembly can reach completion. Native release approval must
then reject the different linked CLI digest. The commands use a fresh folder
under the fixture's ignored `out` directory. Replace the bracketed values with
the local fixture and worktree paths and a private test repository name.

```powershell
& {
    $ErrorActionPreference = 'Stop'
    function Invoke-Checked($program) {
        & $program @args
        if ($LASTEXITCODE -ne 0) { throw "$program $args failed (exit $LASTEXITCODE)." }
    }
    $fixture = '<retained PR #14 offline fixture>'
    $old = '<retained PR #13 worktree>\out\relink-20261031-1151\bootstrap'
    $new = '<retained PR #15 worktree>\out\pr15-20261031-1345\bootstrap'
    $kit = "$new/topaz-core-kit.exe"
    $savedRoot = $env:TOPAZ_CORE_KIT_ROOT
    Push-Location -LiteralPath $fixture
    try {
        $env:TOPAZ_CORE_KIT_ROOT = $fixture
        $oldHash = (Get-FileHash -LiteralPath "$old/generated_cli.rs" -Algorithm SHA256).Hash.ToLowerInvariant()
        $newHash = (Get-FileHash -LiteralPath "$new/generated_cli.rs" -Algorithm SHA256).Hash.ToLowerInvariant()
        $id = Invoke-Checked $kit kit build-id
        if ($id -cne $newHash -or $id -ceq $oldHash) { throw 'Expected distinct source hashes and the PR #15 build ID.' }

        $case = 'out/build-id-negative-' + [guid]::NewGuid().ToString('N')
        $build = "$case/build"
        New-Item -ItemType Directory -Path $build | Out-Null
        foreach ($name in 'cli.request.json','cli.response.json','generated_cli.rs') {
            Copy-Item -LiteralPath "$old/$name" -Destination $build
        }
        Copy-Item -LiteralPath $kit -Destination $build
        $recipe = 'core-kit/distribution/windows-x86_64-development.json'
        foreach ($candidate in 'a','b') {
            Invoke-Checked $kit kit payload-assemble --build-dir $build --recipe $recipe --out-dir "$case/$candidate"
        }
        Invoke-Checked $kit kit payload-compare --payload-dir "$case/a" --other "$case/b"

        $revision = Invoke-Checked git rev-parse HEAD
        $reason = 'the candidate executable was not linked from the recorded generated_cli.rs'
        foreach ($command in 'payload-promote','payload-pack') {
            $destination = "$case/$command"
            $extra = @()
            if ($command -eq 'payload-pack') {
                $extra = @('--repository','<owner/name>','--tag','pr15-negative-fixture','--source-commit',$revision)
            }
            $ErrorActionPreference = 'Continue'
            $messages = & $kit kit $command --payload-dir "$case/a" --other "$case/b" --out-dir $destination @extra 2>&1
            $code = $LASTEXITCODE
            $ErrorActionPreference = 'Stop'
            $messages | Out-File -LiteralPath "$case/$command.log" -Encoding UTF8
            if ($code -ne 1 -or -not ($messages -join "`n").Contains($reason) -or (Test-Path -LiteralPath $destination)) {
                throw "Unexpected result for $command. Inspect $case/$command.log."
            }
        }
        Write-Output "Linked-source mismatch refused by promotion and packaging. Evidence in $fixture/$case"
    } finally {
        [Environment]::SetEnvironmentVariable('TOPAZ_CORE_KIT_ROOT', $savedRoot, 'Process')
        Pop-Location
    }
}
```

### Additional installer checks

1. With the checked-in header-only catalogue, check that the installer reports
   an unavailable target before making a network request. Check the launcher
   also reports the unavailable target.
2. Repeat the fixture installation from the same archive, verify the installed
   bytes, and run the launcher from another working directory. Record argument
   forwarding and exit codes for paths containing spaces.
3. Change a CLI source only in a disposable matching test checkout. Confirm
   an installed launcher can still run source maintenance and local build
   commands, while explicit `kit install --check` rejects the changed input
   identity. Restore the fixture without changing the retained evidence.
4. Reject a copied archive with a changed byte or wrong length before candidate
   execution. Reject malformed catalogue records, a missing native target,
   mismatched input sets and incomplete receipts in isolated fixtures. Also
   change an installed companion and rewrite its local manifest consistently.
   Explicit installation checking must still reject the reconstructed archive
   identity against the unchanged catalogue.
5. Interrupt only a dedicated test installation while staging. Confirm that
   no incomplete stage is selected, an earlier completed version remains
   intact, and the reported stage and lock support diagnosis.
6. With two valid private test versions, select A, then switch to a
   not-yet-installed B's matching source checkout and catalogue. The launcher
   must report B as unavailable without running A. Install B, check its
   selection, and restore A's matching source
   checkout and catalogue to confirm that its retained installation is selected.

Keep successful installation and negative test results distinct. Controlled
archive-format fixtures can exercise links, duplicate names, invalid modes,
malformed headers, trailing bytes and decompression bounds. Use an isolated
fixture with genuine locally generated expected hashes. Such fixtures do not
create an approved public asset or justify adding placeholder catalogue rows.

Return the version directory, `installation.data`, selected catalogue row,
archive/executable hashes, command outputs and first failure. Do not upload or
publish an asset as part of local validation. Linux execution and script
parsing elsewhere do not replace this Windows gate or establish macOS support.

### First run on an ordinary Windows host

Use the PR #15 archive under `out/private-package` and the matching checkout
from the retained PR #15 offline fixture.
The package receipt records the archive hash for transfer.
[Archive identity and result](VALIDATION.md#pr-15-windows-build-and-offline-installation)

Copy the modified fixture's complete source tree to a separate Windows x64
host without Rust or Visual Studio. Include the refreshed tracked inventory,
recipe and test `core-kit/downloads.data`. Exclude `.git`, `out` and any
`compiler/target` cache. Transfer the measured archive separately and pass its
absolute path to `install-core-kit.cmd --archive` from that fresh checkout.
Record the Windows version, architecture and any separately installed Visual
C++ runtime. The Windows development recipe allows `vcruntime140.dll`.

Run the actual first-stage installer there, followed by version, source and
project checks. The initial executable is extracted alone. Packaging therefore
rejects imports satisfied only by bundled runtime files, while the platform
record must explain any runtime the host must provide before this first run.
Use a separate test environment rather than removing DLLs from the development
machine. Preserve the completed PR #8 comparison and current build cache.

## Separate release checks

Canonical regeneration is requested with `bootstrap --verify-core` or
`bootstrap.cmd --verify-core`. The owner has already reported its successful
byte comparison for the recorded canonical inputs. Repeat it when those inputs
change or an independent comparison is needed, not after unrelated CLI work.

Complete payload reproduction needs independent artifacts from the same source
inputs and intended profile, plus compiler, linker, SDK and environment records.
The POSIX procedure in [REPRODUCIBILITY.md](REPRODUCIBILITY.md) does not establish
a Windows linker recipe or another platform's support.

Assemble each separately recorded build and compare the complete candidates.

```text
topaz-core-kit kit payload-assemble --build-dir <build> --recipe <repository-file> --out-dir <new-candidate>
topaz-core-kit kit payload-compare --payload-dir <first> --other <second>
```

Assembly checks current CLI input, successful compiler response, generated
source, native image, dependencies and required companions. Comparison covers
all bytes, manifests and modes. Matching directories still need independent
construction histories to support a reproduction claim.

The Linux and Windows x86-64 development recipes have `releaseReady` false
and must reject promotion and packaging. The per-target release recipes are
the approved inputs for catalogue rows. Record distribution-command execution
in [VALIDATION.md](VALIDATION.md) instead of inferring it from these
instructions.

Approved candidates can be staged or packaged on their native target.

```text
topaz-core-kit kit payload-promote --payload-dir <first> --other <second> --out-dir <new-stage>
topaz-core-kit kit payload-pack --payload-dir <first> --other <second> --out-dir <new-release> --repository studiohaze/topaz --tag <release-tag> --source-commit <40-character-commit>
```

Both apply the native version, source, project and recorded-input gates and
recompare unchanged candidates. Promotion copies them into a fresh explicit
stage. Packaging writes a deterministic archive, catalogue row and package
receipt. No build, Git commit, release upload or installation is implicit.

Final distribution needs reviewed platform requirements, component notices,
complete independent reproduction and actual installation evidence. The
profile admits files up to 256 MiB and a complete raw tar up to 256 MiB.
Received archives are bounded at 512 MiB. The PR #15 Windows archive passed
offline installation and installed-byte verification on the development machine.
First installation on a clean host and builds with independent caches remain open.
