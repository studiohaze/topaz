# Normalize source locations for a separately verified Windows payload build.
# This is the ordinary bootstrap with fixed Rust path mappings, not a claim
# that this host/toolchain or its complete output has been reproduced.
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
$ErrorActionPreference = 'Stop'

function Show-Usage {
    'usage: core-kit\repro-build.ps1 [OUTPUT_DIRECTORY]'
    'Builds Core Kit with stable Rust source paths; does not regenerate the core.'
    'For a reproduction check, run in two clean checkouts and compare their actual executables.'
}

if ($Arguments.Count -gt 1) { Show-Usage | ForEach-Object { [Console]::Error.WriteLine($_) }; exit 2 }
if ($Arguments.Count -eq 1) {
    if ($Arguments[0] -eq '-h' -or $Arguments[0] -eq '--help') { Show-Usage; exit 0 }
    if ($Arguments[0].StartsWith('-')) { Show-Usage | ForEach-Object { [Console]::Error.WriteLine($_) }; exit 2 }
}

$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..')).TrimEnd('\')
$out = if ($Arguments.Count -eq 1) { $Arguments[0] } else { "$root\out\core-kit-repro" }

# An inherited flags string could override the mapping or change the recipe.
# Fail explicitly rather than silently discarding a caller's compiler flags.
if ($env:RUSTFLAGS -or $env:CARGO_ENCODED_RUSTFLAGS) {
    [Console]::Error.WriteLine('Unset RUSTFLAGS and CARGO_ENCODED_RUSTFLAGS before using this fixed path-mapping recipe.')
    exit 1
}

New-Item -ItemType Directory -Force -Path $out | Out-Null
$out = (Resolve-Path -LiteralPath $out).ProviderPath.TrimEnd('\')
$generated = Get-Item -LiteralPath "$out\generated_cli.rs" -Force -ErrorAction SilentlyContinue
if ($generated -and ($generated.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    [Console]::Error.WriteLine('The generated CLI output must be a regular file, not a link.')
    exit 1
}

# Cargo splits this variable only at U+001F. Spaces, Unicode and '=' in source
# paths remain within a single rustc argument. The exact generated-file rule
# comes last so it also normalizes a custom output outside the checkout.
# bootstrap.cmd is started through the same root string, so the paths Cargo
# passes to rustc begin with exactly this prefix.
$separator = [string][char]0x1F
$env:CARGO_ENCODED_RUSTFLAGS = "--remap-path-prefix=$root=/topaz-core-kit$separator--remap-path-prefix=$out\generated_cli.rs=/topaz-core-kit/generated/cli.rs"

'Source path recipe: checkout -> /topaz-core-kit; generated CLI -> /topaz-core-kit/generated/cli.rs.'
'Changing compiler flags can rebuild the core dependency even when canonical self-regeneration is disabled.'
# Windows PowerShell 5.1 turns redirected native stderr into error records.
# Cargo progress uses stderr, so only the exit status decides the result.
# The flags apply to this build only. A script run in an existing session
# removes them again so a later run starts from the same refusal check.
$ErrorActionPreference = 'Continue'
try {
    & "$root\bootstrap.cmd" --out $out
    $code = $LASTEXITCODE
} finally {
    Remove-Item Env:CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue
}
exit $code
