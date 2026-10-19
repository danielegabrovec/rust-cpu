#requires -Version 7.0
param([switch]$Release)
$ErrorActionPreference = 'Stop'
Push-Location -LiteralPath (Split-Path -Parent $PSScriptRoot)
try {
    cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { throw 'Formattazione fallita' }
    cargo clippy --all-targets --locked -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy fallito' }
    cargo test --all-targets --locked
    if ($LASTEXITCODE -ne 0) { throw 'Test falliti' }
    if ($Release) { cargo build --release --locked } else { cargo build --locked }
    if ($LASTEXITCODE -ne 0) { throw 'Build fallita' }
    $binary = if ($Release) { '.\target\release\rust-cpu.exe' } else { '.\target\debug\rust-cpu.exe' }
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss-ffff'
    & $binary --snapshot "test-output\snapshot-$stamp.json" --record "test-output\session-$stamp.csv" --samples 3 --no-llm
    if ($LASTEXITCODE -ne 0) { throw 'Smoke test dati reali fallito' }
    & $binary --render "test-output\overview-$stamp.txt" --no-llm
    if ($LASTEXITCODE -ne 0) { throw 'Smoke test TUI fallito' }
    Write-Host 'Verifiche completate. I report locali non sono versionati.'
} finally { Pop-Location }
