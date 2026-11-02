param([Parameter(Mandatory=$true)][string]$SourceRoot,
      [Parameter(Mandatory=$true)][string]$TargetDirectory,
      [string]$GeneratedDirectory = '',
      [string]$WrapperDirectory = '')
$separator = [char]31
$cargoDirectory = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) '.cargo' }
$mappings = @(@($SourceRoot, '/topaz/source'), @($TargetDirectory, '/topaz/target'), @($cargoDirectory, '/cargo'))
if ($env:OUT_DIR) { $mappings += ,@($env:OUT_DIR, '/topaz/out') }
if ($GeneratedDirectory) { $mappings += ,@($GeneratedDirectory, '/topaz/generated') }
$flags = @()
if ($env:CARGO_ENCODED_RUSTFLAGS) { $flags += $env:CARGO_ENCODED_RUSTFLAGS }
foreach ($mapping in $mappings) {
    foreach ($spelling in @($mapping[0], $mapping[0].Replace('\', '/')) | Select-Object -Unique) {
        $flags += "--remap-path-prefix=$spelling=$($mapping[1])"
    }
}
$env:CARGO_ENCODED_RUSTFLAGS = $flags -join $separator
if ($WrapperDirectory) {
    $wrapperOutputDirectory = Join-Path $TargetDirectory 'topaz-build-tools'
    New-Item -ItemType Directory -Force -Path $wrapperOutputDirectory | Out-Null
    $wrapper = Join-Path $wrapperOutputDirectory 'release-rustc-wrapper.exe'
    $topazRemapCsc = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
    if (!(Test-Path -LiteralPath $topazRemapCsc)) { throw 'Windows .NET Framework C# compiler is required for release path mapping.' }
    & $topazRemapCsc /nologo /target:exe "/out:$wrapper" (Join-Path $WrapperDirectory 'release-rustc-wrapper.cs')
    if ($LASTEXITCODE -ne 0) { throw 'The release path wrapper could not be built.' }
    $env:RUSTC_WRAPPER = $wrapper
}
