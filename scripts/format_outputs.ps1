#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$JsonIn,
    [Parameter(Mandatory = $true, Position = 1)]
    [ValidateSet('json', 'text', 'sarif')]
    [string]$Format,
    [Parameter(Position = 2)]
    [string]$Out = $JsonIn
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $JsonIn)) {
    throw "Input JSON not found: $JsonIn"
}

switch ($Format) {
    'json' {
        Copy-Item -LiteralPath $JsonIn -Destination $Out -Force
    }
    'text' {
        $data = Get-Content -LiteralPath $JsonIn -Raw | ConvertFrom-Json
        $lines = @("Host: $($data.host ?? 'combined')", 'Checks:')
        foreach ($check in @($data.checks)) {
            $lines += "- $($check.id ?? '<id>'): $($check.value)"
        }
        Set-Content -LiteralPath $Out -Value ($lines -join [Environment]::NewLine) -Encoding utf8
    }
    'sarif' {
        $sarif = [ordered]@{
            schema  = 'https://schemastore.azurewebsites.net/schemas/json/sarif-2.1.0.json'
            version = '2.1.0'
            runs    = @(
                [ordered]@{
                    tool    = [ordered]@{ driver = [ordered]@{ name = 'OpenClawScanner' } }
                    results = @()
                }
            )
        }
        Set-Content -LiteralPath $Out -Value ($sarif | ConvertTo-Json -Depth 8) -Encoding utf8
    }
}