# Shared mechanical catalogue selection. The catalogue is never executed.
Set-StrictMode -Version 2.0

function Get-TopazRegularFile {
    param([string]$Path)
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw [IO.InvalidDataException]::new("Expected a regular Core Kit file without a reparse point: $Path")
    }
    return $item
}

function Assert-TopazDirectory {
    param([string]$Path)
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw [IO.InvalidDataException]::new("Expected a Core Kit directory without a reparse point: $Path")
    }
}

function Get-TopazHostTarget {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        throw 'Use ./install-core-kit and ./topaz on this operating system.'
    }
    $architecture = $env:PROCESSOR_ARCHITECTURE
    if (-not [string]::IsNullOrEmpty($env:PROCESSOR_ARCHITEW6432)) {
        $architecture = $env:PROCESSOR_ARCHITEW6432
    }
    if ([string]::IsNullOrEmpty($architecture)) {
        throw 'Windows did not report an architecture. See core-kit/BUILD.md for source builds.'
    }
    switch ($architecture.ToUpperInvariant()) {
        'AMD64' { return 'x86_64-pc-windows-msvc' }
        'ARM64' { return 'aarch64-pc-windows-msvc' }
        default { throw 'No Core Kit download target for this Windows architecture. See core-kit/BUILD.md for source builds.' }
    }
}

function Test-TopazDownloadToken {
    param([string]$Value)
    return $Value.Length -ge 1 -and $Value.Length -le 128 -and
        $Value -cmatch '^[A-Za-z0-9][A-Za-z0-9._-]*$' -and -not $Value.EndsWith('.')
}

function Get-TopazDownload {
    param([string]$Root, [string]$Target)
    Assert-TopazDirectory (Join-Path $Root 'core-kit')
    $path = Join-Path $Root 'core-kit/downloads.data'
    $item = Get-TopazRegularFile $path
    if ($item.Length -lt 1 -or $item.Length -gt 65536) {
        throw 'The download catalogue must contain between 1 and 65536 bytes.'
    }
    $bytes = [IO.File]::ReadAllBytes($item.FullName)
    if ($bytes.Length -ne $item.Length -or $bytes[$bytes.Length - 1] -ne 10) {
        throw 'The download catalogue changed or lacks its final LF.'
    }
    foreach ($byte in $bytes) {
        if ($byte -ne 10 -and ($byte -lt 32 -or $byte -gt 126)) {
            throw 'The download catalogue must use plain ASCII with LF line endings.'
        }
    }
    $lines = [Text.Encoding]::ASCII.GetString($bytes).Split([char]10)
    if ($lines[0] -cne 'topaz.core-kit-downloads/v1' -or $lines.Length -gt 8) {
        throw 'Unsupported Core Kit download catalogue or too many target rows.'
    }
    $targets = @('aarch64-apple-darwin', 'aarch64-pc-windows-msvc', 'aarch64-unknown-linux-gnu',
        'x86_64-apple-darwin', 'x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu')
    $previous = ''
    $selected = $null
    for ($i = 1; $i -lt ($lines.Length - 1); $i++) {
        $fields = $lines[$i].Split([char]'|')
        if ($fields.Length -ne 10 -or -not ($targets -ccontains $fields[0])) {
            throw 'A catalogue row must have ten fields and a supported target.'
        }
        if ($previous -ne '' -and [string]::CompareOrdinal($fields[0], $previous) -le 0) {
            throw 'Catalogue targets must be sorted and unique.'
        }
        $previous = $fields[0]
        $repository = $fields[1].Split([char]'/')
        if ($repository.Length -ne 2 -or -not (Test-TopazDownloadToken $repository[0]) -or
            -not (Test-TopazDownloadToken $repository[1])) {
            throw 'Invalid GitHub repository in the Core Kit download catalogue.'
        }
        if (-not (Test-TopazDownloadToken $fields[2]) -or
            [string]::Equals($fields[2], 'latest', [StringComparison]::OrdinalIgnoreCase)) {
            throw 'A catalogue release tag must be a fixed portable name.'
        }
        if ($fields[3] -cnotmatch '^[0-9a-f]{40}$') { throw 'Invalid catalogue source commit.' }
        foreach ($index in @(4, 7, 9)) {
            if ($fields[$index] -cnotmatch '^[0-9a-f]{64}$') { throw 'Invalid catalogue SHA-256.' }
        }
        if ($fields[5] -cne ('topaz-core-kit-' + $fields[0] + '.tar.gz')) {
            throw 'The release asset name does not match its target.'
        }
        if ($fields[6] -cnotmatch '^[1-9][0-9]{0,8}$' -or [long]$fields[6] -gt 536870912 -or
            $fields[8] -cnotmatch '^[1-9][0-9]{0,8}$' -or [long]$fields[8] -gt 268435456) {
            throw 'A catalogue byte count is invalid or exceeds its limit.'
        }
        if ($fields[0] -ceq $Target) {
            $selected = [pscustomobject]@{
                Target = $fields[0]; Repository = $fields[1]; Tag = $fields[2]
                SourceCommit = $fields[3]; InputSetSha256 = $fields[4]; AssetName = $fields[5]
                AssetBytes = [long]$fields[6]; AssetSha256 = $fields[7]
                ExecutableBytes = [long]$fields[8]; ExecutableSha256 = $fields[9]
            }
        }
    }
    if ($null -eq $selected) {
        throw "Missing platform row: this checkout has no Core Kit archive for $Target. Build from source with .\bootstrap.cmd."
    }
    return $selected
}

function Assert-TopazFileIdentity {
    param([string]$Path, [long]$Bytes, [string]$Sha256, [string]$Kind = 'Download')
    $item = Get-TopazRegularFile $Path
    if ($item.Length -ne $Bytes) { throw "$Kind size mismatch: use the archive selected by this catalogue, or install from a later commit containing the matching catalogue rows." }
    $stream = [IO.File]::Open($item.FullName, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $hash = [Security.Cryptography.SHA256]::Create()
    try {
        if ($stream.Length -ne $Bytes) { throw 'The selected file changed before verification.' }
        $actual = [BitConverter]::ToString($hash.ComputeHash($stream)).Replace('-', '').ToLowerInvariant()
        if ($stream.Length -ne $Bytes -or $actual -cne $Sha256) {
            throw "$Kind hash mismatch: use the archive selected by this catalogue, or install from a later commit containing the matching catalogue rows."
        }
    } finally {
        $hash.Dispose()
        $stream.Dispose()
    }
}

function Get-TopazInstalledRelativePath {
    param([string]$Root, $Download)
    $path = $Root
    foreach ($component in @('out', 'core-kit-installed', $Download.Target, $Download.AssetSha256)) {
        $path = Join-Path $path $component
        try { Assert-TopazDirectory $path }
        catch [System.Management.Automation.ItemNotFoundException] {
            throw 'This Core Kit version is not installed. Run .\install-core-kit.cmd.'
        }
    }
    try {
        $receipt = Get-TopazRegularFile (Join-Path $path 'installation.data')
        $expected = 'topaz.core-kit-installation/v1|' + $Download.Target + '|' +
            $Download.AssetSha256 + '|' + $Download.InputSetSha256 + "`n"
        $matchesReceipt = $receipt.Length -eq $expected.Length
        if ($matchesReceipt) {
            $bytes = [IO.File]::ReadAllBytes($receipt.FullName)
            $matchesReceipt = [string]::Equals([Text.Encoding]::ASCII.GetString($bytes), $expected, [StringComparison]::Ordinal)
        }
        if (-not $matchesReceipt) {
            throw [IO.InvalidDataException]::new('The local Core Kit installation receipt does not match this checkout catalogue.')
        }
        $payload = Join-Path $path 'payload'
        Assert-TopazDirectory $payload
        $null = Get-TopazRegularFile (Join-Path $payload 'topaz-core-kit.exe')
    } catch [System.Management.Automation.ItemNotFoundException], [IO.InvalidDataException] {
        throw "$($_.Exception.Message)`nTo reinstall, move this version directory aside, then run .\install-core-kit.cmd:`n$path"
    }
    # Only ASCII generated from validated fields crosses back to cmd.exe.
    return ('out\core-kit-installed\' + $Download.Target + '\' + $Download.AssetSha256 + '\payload\topaz-core-kit.exe')
}

function ConvertTo-TopazWindowsArguments {
    param([string[]]$Values)
    $quoted = foreach ($value in $Values) {
        $builder = New-Object Text.StringBuilder
        [void]$builder.Append('"')
        $slashes = 0
        foreach ($character in $value.ToCharArray()) {
            if ($character -eq [char]92) { $slashes++; continue }
            if ($character -eq [char]34) {
                [void]$builder.Append(('\' * (2 * $slashes + 1)))
                [void]$builder.Append('"')
            } else {
                if ($slashes -gt 0) { [void]$builder.Append(('\' * $slashes)) }
                [void]$builder.Append($character)
            }
            $slashes = 0
        }
        if ($slashes -gt 0) { [void]$builder.Append(('\' * (2 * $slashes))) }
        [void]$builder.Append('"')
        $builder.ToString()
    }
    return ($quoted -join ' ')
}
