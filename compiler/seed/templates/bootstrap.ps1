$ErrorActionPreference = 'Stop'
Push-Location -LiteralPath $PSScriptRoot
try {
  & cargo build --release --locked --offline --jobs 1
  if ($LASTEXITCODE -ne 0) { throw 'The seed build failed.' }
  & ./target/release/topaz-seed.exe --bootstrap . target/bootstrap-regenerated.rs
  if ($LASTEXITCODE -ne 0) { throw 'The compiler could not regenerate its Rust source.' }
  $original = [IO.File]::ReadAllBytes((Join-Path $PSScriptRoot 'generated/compiler.rs'))
  $regenerated = [IO.File]::ReadAllBytes((Join-Path $PSScriptRoot 'target/bootstrap-regenerated.rs'))
  if ($original.Length -ne $regenerated.Length) { throw 'The regenerated Rust source differs.' }
  for ($index = 0; $index -lt $original.Length; $index++) {
    if ($original[$index] -ne $regenerated[$index]) { throw "The regenerated Rust source differs at byte $index." }
  }
  Write-Output 'The compiler regenerated the same Rust source.'
} finally { Pop-Location }
