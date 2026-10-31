$ErrorActionPreference = 'Stop'

function Read-Utf8([string] $Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "missing file: $Path" }
    # Decode bytes directly so BOM detection cannot select another encoding.
    return [Text.UTF8Encoding]::new($false, $true).GetString([IO.File]::ReadAllBytes($Path))
}

function Read-JsonFile([string] $Path) {
    $text = Read-Utf8 $Path
    if ([string]::IsNullOrWhiteSpace($text)) { throw "empty JSON file: $Path" }
    try { return ($text | ConvertFrom-Json -ErrorAction Stop) }
    catch { throw "invalid JSON file ${Path}: $($_.Exception.Message)" }
}

function Hash-File([string] $Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "missing file: $Path" }
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash.ToLowerInvariant()
}

function Assert-Hash([string] $Path, [string] $Expected) {
    if ($Expected -cnotmatch '^[0-9a-f]{64}$') { throw "invalid expected hash: $Path" }
    if ((Hash-File $Path) -cne $Expected) { throw "hash mismatch: $Path" }
}

function Assert-RelativePath([string] $Path) {
    if ($Path -cnotmatch '^[A-Za-z0-9_.\/-]+$' -or
        $Path -cmatch '^/|/$|//|(^|/)\.\.?($|/)') {
        throw "invalid inventory path: $Path"
    }
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$inventory = Read-Utf8 (Join-Path $root 'core-kit\verify-core.data')
$metadata = [Collections.Generic.Dictionary[string,string]]::new([StringComparer]::Ordinal)
$tables = @{}
foreach ($kind in @('compiler_source', 'snapshot_file', 'input_file')) {
    $tables[$kind] = [Collections.Generic.Dictionary[string,string]]::new([StringComparer]::Ordinal)
}
foreach ($line in ($inventory -split "`n")) {
    if ($line.Length -eq 0 -or $line.StartsWith('#', [StringComparison]::Ordinal)) { continue }
    $fields = $line.Split([char]'|')
    $kind = $fields[0]
    if (@('format', 'compiler_source_set', 'compiler_sources', 'snapshot_files', 'input_files') -ccontains $kind) {
        if ($fields.Count -ne 2 -or $metadata.ContainsKey($kind)) { throw 'invalid or duplicate inventory metadata' }
        $metadata.Add($kind, $fields[1])
    } elseif (@('compiler_source', 'snapshot_file', 'input_file') -ccontains $kind) {
        if ($fields.Count -ne 3) { throw 'invalid inventory file record' }
        Assert-RelativePath $fields[1]
        if ($kind -cne 'compiler_source' -and $fields[1] -ieq 'core-kit/downloads.data') {
            throw 'download catalogue must remain outside the recorded build inputs'
        }
        if ($fields[2] -cnotmatch '^[0-9a-f]{64}$' -or $tables[$kind].ContainsKey($fields[1])) {
            throw 'invalid hash or duplicate inventory file'
        }
        $tables[$kind].Add($fields[1], $fields[2])
    } else { throw "unknown verifier inventory record: $kind" }
}
if ($metadata['format'] -cne 'topaz.core-kit-inputs/v2' -or
    $metadata['compiler_source_set'] -cnotmatch '^sha256:[0-9a-f]{64}$') {
    throw 'invalid verifier inventory format or source set'
}
foreach ($pair in @(@('compiler_sources', 'compiler_source'), @('snapshot_files', 'snapshot_file'), @('input_files', 'input_file'))) {
    $count = $metadata[$pair[0]]
    if ($count -cnotmatch '^[1-9][0-9]*$' -or [long]$count -ne $tables[$pair[1]].Count) {
        throw "inventory count mismatch: $($pair[0])"
    }
}
foreach ($path in @('SOURCE-COMPONENTS.json', 'compiler/generated/native/compiler.json',
    'compiler/generated/native/compiler.rs', 'compiler/seed/initial/request.json',
    'compiler/seed/initial/REQUEST-PROVENANCE.json', 'compiler/seed/initial/std/bytes.tpz',
    'compiler/seed/initial/std/json.tpz', 'compiler/seed/initial/std/fs.tpz',
    'compiler/seed/initial/std/io.tpz', 'core-kit/cli/REQUEST.json', 'core-kit/cli/initial/request.json')) {
    if (-not $tables['input_file'].ContainsKey($path)) { throw "required input missing from inventory: $path" }
}

$sourceRoot = Join-Path $root 'compiler\crates\topaz_self_frontend\topaz'
$initialRoot = Join-Path $root 'compiler\seed\initial'
$initialTopaz = Join-Path $initialRoot 'topaz'
foreach ($entry in $tables['compiler_source'].GetEnumerator()) {
    Assert-Hash (Join-Path $sourceRoot $entry.Key) $entry.Value
    Assert-Hash (Join-Path $initialTopaz $entry.Key) $entry.Value
}
foreach ($kind in @('snapshot_file', 'input_file')) {
    foreach ($entry in $tables[$kind].GetEnumerator()) { Assert-Hash (Join-Path $root $entry.Key) $entry.Value }
}

# The common inventory checks exact bytes on both platforms. PowerShell also
# checks the JSON relationships so a coordinated inventory update cannot silently
# discard a source fact or disagree with the authoritative manifest.
$manifest = Read-JsonFile (Join-Path $root 'compiler\generated\native\compiler.json')
if ($manifest.schema -cne 'topaz.native-compiler/v1' -or
    $manifest.sourceSetId -cne $metadata['compiler_source_set'] -or
    @($manifest.sourceFiles).Count -ne $tables['compiler_source'].Count -or
    $manifest.generatedRustSha256 -cne ('sha256:' + $tables['input_file']['compiler/generated/native/compiler.rs'])) {
    throw 'compiler manifest differs from verifier inventory'
}
$seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$sourceBytes = 0L
foreach ($row in @($manifest.sourceFiles)) {
    Assert-RelativePath $row.path
    if (-not $seen.Add($row.path) -or -not $tables['compiler_source'].ContainsKey($row.path) -or
        $row.sha256 -cne ('sha256:' + $tables['compiler_source'][$row.path])) {
        throw "compiler source inventory differs: $($row.path)"
    }
    $length = (Get-Item -LiteralPath (Join-Path $sourceRoot $row.path)).Length
    if ($length -ne $row.bytes) { throw "compiler source byte count differs: $($row.path)" }
    $sourceBytes += $length
}
$components = Read-JsonFile (Join-Path $root 'SOURCE-COMPONENTS.json')
if (@($components.snapshot.sourceFiles).Count -ne $tables['snapshot_file'].Count) { throw 'snapshot inventory count differs' }
$seen.Clear()
foreach ($row in @($components.snapshot.sourceFiles)) {
    Assert-RelativePath $row.destination
    if (-not $seen.Add($row.destination) -or -not $tables['snapshot_file'].ContainsKey($row.destination) -or
        $row.sha256 -cne $tables['snapshot_file'][$row.destination]) {
        throw "snapshot inventory differs: $($row.destination)"
    }
}

$request = Read-JsonFile (Join-Path $initialRoot 'request.json')
if ($request.schema -cne 'topaz.compiler.stage1-exchange/v2' -or
    $request.compilerSourceSetId -cne $manifest.sourceSetId) { throw 'invalid seed request schema or source set' }
$expectedSources = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($path in $tables['compiler_source'].Keys) { [void]$expectedSources.Add($path) }
[void]$expectedSources.Add('std/bytes.tpz')
$seen.Clear()
foreach ($fact in @($request.facts)) {
    if ($fact.kind -cne 'read-source') { continue }
    $path = [string]$fact.logicalPath
    if (-not $expectedSources.Contains($path) -or -not $seen.Add($path) -or
        $fact.status -cne 'present' -or $fact.mountId -cne 'root') {
        throw "unexpected or duplicate seed source fact: $path"
    }
    $directory = if ($path.StartsWith('std/', [StringComparison]::Ordinal)) { $initialRoot } else { $initialTopaz }
    if ($fact.source -isnot [string] -or $fact.source -cne (Read-Utf8 (Join-Path $directory $path))) {
        throw "seed request source fact differs: $path"
    }
}
if ($seen.Count -ne $expectedSources.Count) { throw 'seed request omits source facts' }
$modules = @($request.package.generatedStdModules)
$jsonModule = Join-Path $initialRoot 'std\json.tpz'
if ($modules.Count -ne 1 -or $modules[0].path -cne 'std/json.tpz' -or $modules[0].identity -cne 'std.json' -or
    $modules[0].source -isnot [string] -or
    $modules[0].source -cne (Read-Utf8 $jsonModule)) { throw 'seed request embedded std/json.tpz differs' }
$provenance = Read-JsonFile (Join-Path $initialRoot 'REQUEST-PROVENANCE.json')
if ($provenance.schema -cne 'topaz.note.seed-request/v1' -or
    $provenance.compilerSourceSetId -cne $manifest.sourceSetId -or
    $provenance.sourceFiles -ne $tables['compiler_source'].Count -or
    $provenance.sourceBytes -ne $sourceBytes -or
    $provenance.generatedCompilerSha256 -cne $manifest.generatedRustSha256 -or
    $provenance.standardModules.'std/json.tpz' -cne (Hash-File $jsonModule) -or
    $provenance.standardModules.'std/bytes.tpz' -cne (Hash-File (Join-Path $initialRoot 'std\bytes.tpz'))) {
    throw 'seed request provenance differs from actual inputs'
}

$descriptor = Read-JsonFile (Join-Path $root 'core-kit\cli\REQUEST.json')
if ($descriptor.schema -cne 'topaz.note.core-kit-cli-request/v2' -or
    $descriptor.compilerSourceSetId -cne $manifest.sourceSetId) { throw 'invalid CLI input descriptor' }
Assert-RelativePath $descriptor.requestFile
if (-not $tables['input_file'].ContainsKey($descriptor.requestFile)) { throw 'CLI request is not in the input inventory' }
$cliRequest = Read-JsonFile (Join-Path $root $descriptor.requestFile)
if ($cliRequest.schema -cne 'topaz.compiler.stage1-exchange/v2' -or
    $cliRequest.compilerSourceSetId -cne $manifest.sourceSetId -or
    $cliRequest.terminal -cne 'rust-source') { throw 'CLI request schema or compiler identity differs' }
$cliSources = [Collections.Generic.Dictionary[string,string]]::new([StringComparer]::Ordinal)
foreach ($mapping in @($descriptor.sourceFiles)) {
    Assert-RelativePath $mapping.logicalPath
    Assert-RelativePath $mapping.path
    if ($cliSources.ContainsKey($mapping.logicalPath) -or -not $tables['input_file'].ContainsKey($mapping.path)) {
        throw "CLI source mapping is duplicate or unrecorded: $($mapping.logicalPath)"
    }
    $cliSources.Add($mapping.logicalPath, $mapping.path)
}
if ($cliSources.Count -eq 0 -or -not $cliSources.ContainsKey($cliRequest.entry)) { throw 'CLI input entry is missing' }
$seen.Clear()
foreach ($fact in @($cliRequest.facts)) {
    if ($fact.kind -cne 'read-source') { continue }
    $path = [string]$fact.logicalPath
    if (-not $cliSources.ContainsKey($path) -or -not $seen.Add($path) -or
        $fact.mountId -cne 'root' -or $fact.status -cne 'present' -or $fact.source -isnot [string] -or
        $fact.source -cne (Read-Utf8 (Join-Path $root $cliSources[$path]))) {
        throw "CLI request source fact differs: $path"
    }
}
if ($seen.Count -ne $cliSources.Count) { throw 'CLI request omits source facts' }
$cliStd = [Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal)
$seen.Clear()
foreach ($mapping in @($descriptor.standardModules)) {
    Assert-RelativePath $mapping.path
    Assert-RelativePath $mapping.sourceFile
    if ([string]::IsNullOrWhiteSpace($mapping.identity) -or $cliStd.ContainsKey($mapping.identity) -or
        -not $seen.Add($mapping.path) -or -not $tables['input_file'].ContainsKey($mapping.sourceFile)) {
        throw 'CLI standard-module mapping is duplicate or unrecorded'
    }
    $cliStd.Add($mapping.identity, $mapping)
}
if ($cliStd.Count -eq 0 -or @($cliRequest.package.generatedStdModules).Count -ne $cliStd.Count) {
    throw 'CLI standard-module count differs'
}
$seen.Clear()
foreach ($module in @($cliRequest.package.generatedStdModules)) {
    if (-not $cliStd.ContainsKey($module.identity) -or -not $seen.Add($module.identity)) {
        throw 'CLI request contains an unexpected or duplicate standard module'
    }
    $mapping = $cliStd[$module.identity]
    if ($module.path -cne $mapping.path -or $module.source -isnot [string] -or
        $module.source -cne (Read-Utf8 (Join-Path $root $mapping.sourceFile))) {
        throw "CLI request standard module differs: $($module.identity)"
    }
}

Write-Output ('Core Kit input hashes match: {0} compiler source pairs, {1} snapshot files, {2} input files' -f
    $tables['compiler_source'].Count, $tables['snapshot_file'].Count, $tables['input_file'].Count)
Write-Output 'This checks the recorded input bytes; it does not run the compiler or prove reproducibility.'
