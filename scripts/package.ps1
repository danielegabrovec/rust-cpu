#requires -Version 7.0
param([string]$MakeNsis)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Build release fallita' }
    $metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    $version = $metadata.packages[0].version
    & (Join-Path $PSScriptRoot 'third-party-notices.ps1')
    $artifactsDir = Join-Path $projectRoot 'artifacts'
    New-Item -ItemType Directory -Force -Path $artifactsDir | Out-Null
    $stageDir = Join-Path $artifactsDir ("staging-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $stageDir | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot 'target\release\rust-cpu.exe') -Destination $stageDir
    foreach ($file in @('README.md','LICENSE','CHANGELOG.md','THIRD-PARTY-NOTICES.txt')) { Copy-Item -LiteralPath (Join-Path $projectRoot $file) -Destination $stageDir }
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs') -Destination $stageDir -Recurse -Force
    $zipPath = Join-Path $artifactsDir "rust-cpu-$version-windows-x64-portable.zip"
    Compress-Archive -Path (Join-Path $stageDir '*') -DestinationPath $zipPath -Force
    if (-not $MakeNsis) {
        $command = Get-Command makensis -ErrorAction SilentlyContinue
        if ($command) { $MakeNsis = $command.Source }
        else {
            foreach ($candidate in @('C:\Program Files (x86)\NSIS\makensis.exe', "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe")) {
                if (Test-Path -LiteralPath $candidate) { $MakeNsis = $candidate; break }
            }
        }
    }
    if (-not $MakeNsis) { throw 'NSIS non trovato. Installa NSIS o passa -MakeNsis C:\percorso\makensis.exe' }
    & $MakeNsis "/DVERSION=$version" (Join-Path $projectRoot 'installer\rust-cpu.nsi')
    if ($LASTEXITCODE -ne 0) { throw 'Creazione installer fallita' }
    $outputs = @(Get-Item -LiteralPath $zipPath; Get-Item -LiteralPath (Join-Path $artifactsDir "rust-cpu-$version-windows-x64-setup.exe"))
    $lines = foreach ($output in $outputs) { $hash = Get-FileHash -LiteralPath $output.FullName -Algorithm SHA256; '{0}  {1}' -f $hash.Hash.ToLowerInvariant(),$output.Name }
    $lines | Set-Content -LiteralPath (Join-Path $artifactsDir 'SHA256SUMS.txt') -Encoding ascii
    $outputs | Select-Object Name,Length
} finally { Pop-Location }
