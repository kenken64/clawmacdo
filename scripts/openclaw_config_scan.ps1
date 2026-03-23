#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [string]$OutFile = (Join-Path ([System.IO.Path]::GetTempPath()) ("openclaw_config_scan_{0}.json" -f [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()))
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$base = '/root/.openclaw'
$files = @(
    (Join-Path $base 'openclaw.json'),
    (Join-Path $base 'openclaw.json.bak'),
    (Join-Path $base 'openclaw.json.bak.3'),
    (Join-Path $base 'workspace/MEMORY.md'),
    (Join-Path $base 'AGENTS.md'),
    (Join-Path $base 'SOUL.md')
)

$foundFiles = New-Object System.Collections.Generic.List[string]
$secrets = New-Object System.Collections.Generic.List[string]

foreach ($file in $files) {
    if (-not (Test-Path -LiteralPath $file)) { continue }
    $foundFiles.Add($file)
    $content = Get-Content -LiteralPath $file -Raw -ErrorAction SilentlyContinue
    if ($content -match '(TOKEN|KEY|SECRET|PASSWORD|passwd|api_key|apiKey)') {
        $secrets.Add($file)
    }
}

$writableFiles = New-Object System.Collections.Generic.List[string]
foreach ($dir in @($base, (Join-Path $base 'workspace'))) {
    if (-not (Test-Path -LiteralPath $dir)) { continue }
    try {
        $results = & find $dir -maxdepth 2 -type f -perm /o+w 2>$null
        foreach ($result in @($results)) {
            if (-not [string]::IsNullOrWhiteSpace($result)) {
                $writableFiles.Add($result)
            }
        }
    }
    catch {
    }
}

$report = [ordered]@{
    target          = 'openclaw_config'
    files           = ($foundFiles -join [Environment]::NewLine)
    secrets         = ($secrets -join [Environment]::NewLine)
    world_writable  = ($writableFiles -join [Environment]::NewLine)
    has_MEMORY_md   = if (Test-Path -LiteralPath (Join-Path $base 'workspace/MEMORY.md')) { 'yes' } else { 'no' }
}

Set-Content -LiteralPath $OutFile -Value ($report | ConvertTo-Json -Depth 8) -Encoding utf8
Write-Host "Wrote $OutFile"