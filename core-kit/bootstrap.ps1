# Windows PowerShell 5.1 and PowerShell 7; no profile or execution-policy changes.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Show-Usage {
    Write-Output 'usage: bootstrap.cmd [--out DIRECTORY | DIRECTORY] [--verify-core]'
    Write-Output 'Requires already-installed Rust/Cargo 1.96.0 and the native linker.'
    Write-Output '--verify-core additionally performs expensive canonical core regeneration.'
}

$root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$out = Join-Path $root 'out\core-kit-bootstrap'
$outSet = $false
$verifyCore = $false
for ($i = 0; $i -lt $args.Count; $i++) {
    $argument = [string]$args[$i]
    if ($argument -eq '--help' -or $argument -eq '-h') { Show-Usage; exit 0 }
    if ($argument -eq '--verify-core') {
        if ($verifyCore) { Show-Usage; exit 2 }
        $verifyCore = $true
    } elseif ($argument -eq '--out') {
        if ($outSet -or ($i + 1) -ge $args.Count) { Show-Usage; exit 2 }
        $i++
        $out = [string]$args[$i]
        $outSet = $true
    } elseif ($argument.StartsWith('-') -or $outSet) {
        Show-Usage; exit 2
    } else {
        $out = $argument
        $outSet = $true
    }
}
if ([string]::IsNullOrWhiteSpace($out)) { Show-Usage; exit 2 }
$provider = $null
$drive = $null
$out = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($out, [ref]$provider, [ref]$drive)
if ($provider.Name -ne 'FileSystem') { throw 'Output must be a filesystem path.' }

$environmentNames = @('RUSTUP_TOOLCHAIN', 'RUSTUP_AUTO_INSTALL', 'CARGO_NET_OFFLINE', 'TOPAZ_CORE_KIT_GENERATED_CLI', 'TOPAZ_CORE_KIT_ROOT')
$previousEnvironment = @{}
foreach ($name in $environmentNames) {
    $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
$pushed = $false
try {
    $env:RUSTUP_TOOLCHAIN = '1.96.0'
    $env:RUSTUP_AUTO_INSTALL = '0'
    $env:CARGO_NET_OFFLINE = 'true'
    $env:TOPAZ_CORE_KIT_ROOT = $root
    $cargo = (Get-Command cargo -CommandType Application -ErrorAction Stop).Source
    $rustc = (Get-Command rustc -CommandType Application -ErrorAction Stop).Source
    $rustInfo = @(& $rustc -vV)
    if ($LASTEXITCODE -ne 0) { throw 'Rust 1.96.0 must already be installed; automatic installation is disabled.' }
    $release = @($rustInfo | Where-Object { $_ -match '^release: ' } | ForEach-Object { $_.Substring(9) })
    $hostTarget = @($rustInfo | Where-Object { $_ -match '^host: ' } | ForEach-Object { $_.Substring(6) })
    if ($release.Count -ne 1 -or $release[0] -cne '1.96.0') { throw 'This build requires Rust 1.96.0.' }
    if ($hostTarget.Count -ne 1 -or $hostTarget[0] -cnotmatch '^[A-Za-z0-9_-]+$') { throw 'Cannot determine Rust host target.' }
    $hostTarget = $hostTarget[0]
    $cargoVersion = & $cargo --version
    if ($LASTEXITCODE -ne 0 -or $cargoVersion -cnotmatch '^cargo 1\.96\.0 ') { throw 'This build requires Cargo 1.96.0.' }

    New-Item -ItemType Directory -Force -Path $out | Out-Null
    $compiler = Join-Path $root 'compiler'
    $target = Join-Path $compiler 'target'
    $artifacts = Join-Path (Join-Path $target $hostTarget) 'release'
    $suffix = if ($hostTarget -match '-windows-') { '.exe' } else { '' }
    Push-Location -LiteralPath $compiler
    $pushed = $true

    if ($verifyCore) {
        Write-Output 'Bootstrap mode: product build plus optional canonical core regeneration.'
    } else {
        Write-Output 'Bootstrap mode: product build. Canonical core regeneration is disabled.'
    }
    Write-Output '[1/3] Building the public core and CLI generator (offline). The first build can take a while.'
    & $cargo build -p topaz-core-kit-native --bin topaz-core-kit-generate-cli --release --frozen --target $hostTarget --target-dir $target
    if ($LASTEXITCODE -ne 0) { throw "CLI generator build failed (exit $LASTEXITCODE)." }
    Write-Output '[2/3] Compiling the checked-in Topaz CLI source.'
    & (Join-Path $artifacts "topaz-core-kit-generate-cli$suffix") $out
    if ($LASTEXITCODE -ne 0) { throw "CLI generation failed (exit $LASTEXITCODE)." }

    $env:TOPAZ_CORE_KIT_GENERATED_CLI = Join-Path $out 'generated_cli.rs'
    Write-Output '[3/3] Linking the native Core Kit.'
    & $cargo build -p topaz-core-kit-native --bin topaz-core-kit --features native-cli --message-format=short --release --frozen --target $hostTarget --target-dir $target
    if ($LASTEXITCODE -ne 0) { throw "Core Kit link failed (exit $LASTEXITCODE)." }
    $product = Join-Path $out "topaz-core-kit$suffix"
    Copy-Item -LiteralPath (Join-Path $artifacts "topaz-core-kit$suffix") -Destination $product -Force

    if ($verifyCore) {
        Write-Output 'Regenerating the canonical core; this optional step can take a long time.'
        & $cargo build -p topaz_seed --release --frozen --target $hostTarget --target-dir $target
        if ($LASTEXITCODE -ne 0) { throw "Seed build failed (exit $LASTEXITCODE)." }
        $regenerated = Join-Path $out 'compiler-regenerated.rs'
        & (Join-Path $artifacts "topaz-seed$suffix") --bootstrap (Join-Path $root 'compiler\seed\initial') $regenerated
        if ($LASTEXITCODE -ne 0) { throw "Core regeneration failed (exit $LASTEXITCODE)." }
        $expected = (Get-FileHash -LiteralPath (Join-Path $root 'compiler\generated\native\compiler.rs') -Algorithm SHA256).Hash
        $actual = (Get-FileHash -LiteralPath $regenerated -Algorithm SHA256).Hash
        if ($expected -cne $actual) { throw 'Canonical core regeneration differs.' }
        Write-Output 'Canonical core regeneration matches byte for byte.'
    }
    Write-Output "Core Kit output: $product"
    Write-Output 'Keep the binary inside this checkout, or set TOPAZ_CORE_KIT_ROOT to its data checkout.'
    Write-Output 'No release payload or reproducibility claim is created by this local build.'
} finally {
    if ($pushed) { Pop-Location }
    foreach ($name in $environmentNames) {
        [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], 'Process')
    }
}
