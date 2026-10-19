#requires -Version 7.0
param([string]$Installer)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    if (-not $Installer) { $metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json; $Installer = "artifacts\rust-cpu-$($metadata.packages[0].version)-windows-x64-setup.exe" }
    $registryKey = 'HKCU:\Software\RUST-CPU'
    $uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\RUST-CPU'
    $startMenu = Join-Path ([Environment]::GetFolderPath('Programs')) 'RUST-CPU'
    if ((Test-Path -LiteralPath $registryKey) -or (Test-Path -LiteralPath $uninstallKey) -or (Test-Path -LiteralPath $startMenu)) {
        throw 'Esiste una installazione o collegamento RUST-CPU. Il test evita di sostituirlo.'
    }
    $testRoot = Join-Path $projectRoot 'test-output'
    New-Item -ItemType Directory -Force -Path $testRoot | Out-Null
    $installDir = Join-Path $testRoot ('install-' + [guid]::NewGuid().ToString('N'))
    $installerPath = (Resolve-Path -LiteralPath $Installer).Path
    $process = Start-Process -FilePath $installerPath -ArgumentList "/S /D=$installDir" -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Installer exit code $($process.ExitCode)" }
    $executable = Join-Path $installDir 'rust-cpu.exe'
    $uninstaller = Join-Path $installDir 'Uninstall.exe'
    foreach ($file in @($executable, $uninstaller, (Join-Path $installDir 'docs\MANUALE.md'), (Join-Path $installDir 'docs\screenshots\overview.png'), (Join-Path $installDir 'THIRD-PARTY-NOTICES.txt'))) {
        if (-not (Test-Path -LiteralPath $file)) { throw "File installato assente: $file" }
    }
    & $executable --version
    if ($LASTEXITCODE -ne 0) { throw 'Eseguibile installato non avviabile' }
    $reportPath = Join-Path $testRoot ('installed-' + [guid]::NewGuid().ToString('N') + '.json')
    $databasePath = Join-Path $testRoot ('installed-' + [guid]::NewGuid().ToString('N') + '.sqlite3')
    & $executable --database $databasePath --save-session 'Installer smoke' --samples 2 --snapshot $reportPath --no-llm --interval-ms 500
    if ($LASTEXITCODE -ne 0) { throw 'Snapshot da eseguibile installato fallito' }
    $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    if ($report.snapshot.memory_total -le 0 -or $report.snapshot.processes.Count -eq 0) { throw 'Telemetria installata non valida' }
    & $executable --database $databasePath --db-check
    if ($LASTEXITCODE -ne 0) { throw 'Integrita SQLite installata fallita' }
    $process = Start-Process -FilePath $uninstaller -ArgumentList "/S _?=$installDir" -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Disinstallazione exit code $($process.ExitCode)" }
    # _?= keeps the uninstaller in place so Wait observes the actual uninstall process.
    # Remove only that known file and then an empty directory, never a recursive tree.
    $resolvedInstallDir = [System.IO.Path]::GetFullPath($installDir)
    $resolvedTestRoot = [System.IO.Path]::GetFullPath($testRoot) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $resolvedInstallDir.StartsWith($resolvedTestRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Percorso test esterno alla workspace' }
    if (Test-Path -LiteralPath $uninstaller) { Remove-Item -LiteralPath $uninstaller -Force }
    if (Test-Path -LiteralPath $installDir) { [System.IO.Directory]::Delete($installDir, $false) }
    if ((Test-Path -LiteralPath $registryKey) -or (Test-Path -LiteralPath $uninstallKey) -or (Test-Path -LiteralPath $startMenu) -or (Test-Path -LiteralPath $installDir)) { throw 'Residui installazione rilevati' }
    if (-not (Test-Path -LiteralPath $databasePath)) { throw 'La disinstallazione ha eliminato i dati utente' }
    Write-Host 'Installer verificato: installazione, sessione SQLite reale, integrita, disinstallazione e dati conservati.'
} finally { Pop-Location }
