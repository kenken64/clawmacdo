#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [string]$OutFile,
    [string]$SummaryFile
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ts = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
if (-not $OutFile) {
    $OutFile = Join-Path ([System.IO.Path]::GetTempPath()) "openclaw_security_scan_${ts}.json"
}
if (-not $SummaryFile) {
    $SummaryFile = Join-Path ([System.IO.Path]::GetTempPath()) "openclaw_security_scan_${ts}.summary.txt"
}

$ubuntuOut = Join-Path ([System.IO.Path]::GetTempPath()) "scan_ubuntu_${ts}.json"
$macOut = Join-Path ([System.IO.Path]::GetTempPath()) "scan_macos_${ts}.json"
$configOut = Join-Path ([System.IO.Path]::GetTempPath()) "openclaw_config_scan_${ts}.json"

foreach ($job in @(
    @{ Path = (Join-Path $PSScriptRoot 'ubuntu_scan.ps1'); Args = @{ OutFile = $ubuntuOut } },
    @{ Path = (Join-Path $PSScriptRoot 'macos_scan.ps1'); Args = @{ OutFile = $macOut } },
    @{ Path = (Join-Path $PSScriptRoot 'openclaw_config_scan.ps1'); Args = @{ OutFile = $configOut } }
)) {
    try {
        $jobArgs = $job.Args
        & $job.Path @jobArgs
    }
    catch {
    }
}

$parts = @()
foreach ($path in @($ubuntuOut, $macOut, $configOut)) {
    if (Test-Path -LiteralPath $path) {
        $parts += Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
    }
}

if ($parts.Count -gt 0) {
    $combined = [ordered]@{
        generated_at = [DateTimeOffset]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
        parts        = $parts
    }
    Set-Content -LiteralPath $OutFile -Value ($combined | ConvertTo-Json -Depth 12) -Encoding utf8
}
elseif (Test-Path -LiteralPath $ubuntuOut) {
    Copy-Item -LiteralPath $ubuntuOut -Destination $OutFile -Force
}

$summaryLines = @(
    "Security scan run at $([DateTimeOffset]::UtcNow.ToString('o'))",
    "Combined JSON: $OutFile",
    'Components:'
)
if (Test-Path -LiteralPath $ubuntuOut) { $summaryLines += "- Ubuntu: $ubuntuOut" }
if (Test-Path -LiteralPath $macOut) { $summaryLines += "- macOS: $macOut" }
if (Test-Path -LiteralPath $configOut) { $summaryLines += "- OpenClaw config: $configOut" }
Set-Content -LiteralPath $SummaryFile -Value ($summaryLines -join [Environment]::NewLine) -Encoding utf8

$workspaceDir = '/root/.openclaw/workspace'
if ([System.IO.Directory]::Exists($workspaceDir)) {
    try { Copy-Item -LiteralPath $OutFile -Destination (Join-Path $workspaceDir ([System.IO.Path]::GetFileName($OutFile))) -Force } catch {}
    try { Copy-Item -LiteralPath $SummaryFile -Destination (Join-Path $workspaceDir ([System.IO.Path]::GetFileName($SummaryFile))) -Force } catch {}
}

Write-Host "Wrote $OutFile and summary $SummaryFile"