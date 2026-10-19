#requires -Version 7.0
$ErrorActionPreference = 'Stop'
Push-Location -LiteralPath (Split-Path -Parent $PSScriptRoot)
try {
    $metadataJson = cargo metadata --locked --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata fallito' }
    $metadata = $metadataJson | ConvertFrom-Json -AsHashtable
    $builder = [System.Text.StringBuilder]::new()
    [void]$builder.AppendLine('RUST-CPU - Third-party dependency licenses and notices')
    [void]$builder.AppendLine('Generated from Cargo.lock, including platform-specific and development dependencies.')
    foreach ($package in ($metadata.packages | Where-Object { $_.name -ne 'rust-cpu' } | Sort-Object name,version)) {
        [void]$builder.AppendLine(('=' * 78))
        [void]$builder.AppendLine("$($package.name) $($package.version) | $($package.license)")
        if ($package.repository) { [void]$builder.AppendLine($package.repository) }
        $packageDir = Split-Path -Parent $package.manifest_path
        $files = Get-ChildItem -LiteralPath $packageDir -File | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE)([._-]|$)' -and $_.Extension -in @('', '.txt', '.md', '.LICENSE', '.APACHE', '.MIT', '.BSD') }
        foreach ($file in $files) {
            [void]$builder.AppendLine("--- $($file.Name) ---")
            [void]$builder.AppendLine([System.IO.File]::ReadAllText($file.FullName))
        }
        if (-not $files) { [void]$builder.AppendLine('License identifier declared by the package; consult upstream repository for full terms.') }
    }
    $noticeText = (($builder.ToString().Replace("`r`n", "`n") -split "`n" | ForEach-Object { $_.TrimEnd() }) -join "`n").TrimEnd() + "`n"
    [System.IO.File]::WriteAllText((Join-Path (Get-Location) 'THIRD-PARTY-NOTICES.txt'), $noticeText, [System.Text.UTF8Encoding]::new($false))
} finally { Pop-Location }
