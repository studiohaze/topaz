# The cmd entry point retains the caller's original native arguments.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
try {
    if ($args.Count -ne 0) { throw 'The launch resolver accepts no arguments.' }
    . (Join-Path $PSScriptRoot 'downloads.ps1')
    $root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
    $target = Get-TopazHostTarget
    $download = Get-TopazDownload $root $target
    $relative = Get-TopazInstalledRelativePath $root $download
    [Console]::Out.WriteLine($relative)
    exit 0
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
