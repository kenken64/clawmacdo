#!/usr/bin/env pwsh
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$textOut = Join-Path ([System.IO.Path]::GetTempPath()) 'sample_text.txt'

& (Join-Path $root 'scripts/format_outputs.ps1') (Join-Path $root 'fixtures/sample_high_risk.json') text $textOut
$textContent = Get-Content -LiteralPath $textOut -Raw
if ($textContent -notmatch 'sshd_permit_root') {
    throw 'FAIL: formatted text did not include sshd_permit_root'
}

$evaluator = Join-Path $root 'scripts/evaluate_risk.ps1'
$fixture = Join-Path $root 'fixtures/sample_high_risk.json'
pwsh -NoProfile -File $evaluator $fixture | Out-Null
if ($LASTEXITCODE -eq 0) {
    throw 'FAIL: expected non-zero exit'
}

Write-Host 'PASS: evaluator returned non-zero for high risk'
exit 0