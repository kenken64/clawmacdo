#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [string]$OutFile = (Join-Path ([System.IO.Path]::GetTempPath()) ("openclaw_windows_scan_{0}.json" -f [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()))
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scanScript = Join-Path $PSScriptRoot 'windows_scan.ps1'
if (Test-Path -LiteralPath $scanScript) {
    Write-Host 'Windows scan script exists. Copy scripts/windows_scan.ps1 to the Windows host and run: pwsh ./windows_scan.ps1 -OutPath <path>'
    $payload = [ordered]@{
        status  = 'placeholder'
        message = 'windows_scan.ps1 exists and should be run on the target Windows host.'
    }
}
else {
    $payload = [ordered]@{
        status  = 'missing'
        message = 'No windows_scan.ps1 present; create one or run the wrapper with a script.'
    }
}

Set-Content -LiteralPath $OutFile -Value ($payload | ConvertTo-Json -Depth 5) -Encoding utf8
Write-Host "Wrote $OutFile"