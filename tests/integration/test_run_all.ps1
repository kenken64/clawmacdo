#!/usr/bin/env pwsh
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$json = Join-Path ([System.IO.Path]::GetTempPath()) ("openclaw_security_scan_test_{0}.json" -f [DateTimeOffset]::UtcNow.ToUnixTimeSeconds())

& (Join-Path $root 'scripts/run_all_scans.ps1') -OutFile $json

if (-not (Test-Path -LiteralPath $json)) {
    throw 'FAIL: no json produced'
}

$payload = Get-Content -LiteralPath $json -Raw | ConvertFrom-Json
if ($null -eq $payload.PSObject.Properties['parts']) {
    throw "FAIL: parts not found in $json"
}

Write-Host "PASS: $json contains parts"
exit 0