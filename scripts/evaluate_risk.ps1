#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$JsonPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $JsonPath)) {
    throw "JSON input not found: $JsonPath"
}

$data = Get-Content -LiteralPath $JsonPath -Raw | ConvertFrom-Json
$risk = 0
$notes = @()

if ($null -ne $data.PSObject.Properties['secrets'] -and -not [string]::IsNullOrWhiteSpace([string]$data.secrets)) {
    $risk = 2
    $notes = $data.secrets
}

if ($null -ne $data.PSObject.Properties['checks']) {
    foreach ($check in @($data.checks)) {
        if ($check.id -eq 'sshd_permit_root' -and [string]$check.value -eq 'PermitRootLogin yes') {
            $risk = 2
        }
    }
}

$result = [ordered]@{
    time  = [DateTimeOffset]::UtcNow.ToString('o')
    kind  = 'scan_evaluation'
    risk  = $risk
    notes = $notes
}

$result | ConvertTo-Json -Depth 8 -Compress
exit $risk