#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$npmDir = Join-Path $root 'npm'

if ($DryRun) {
    Write-Host '=== DRY RUN MODE ==='
}

$platformPackages = @(
    '@clawmacdo/darwin-arm64',
    '@clawmacdo/linux-x64',
    '@clawmacdo/win32-x64'
)

foreach ($package in $platformPackages) {
    $packageDir = Join-Path $npmDir $package
    $binDir = Join-Path $packageDir 'bin'
    $hasBinary = (Test-Path -LiteralPath $binDir) -and (Get-ChildItem -LiteralPath $binDir -ErrorAction SilentlyContinue | Select-Object -First 1)
    if (-not $hasBinary) {
        Write-Warning "Skipping $package (no binary found in $binDir)"
        continue
    }

    Write-Host "-> Publishing $package..."
    Push-Location $packageDir
    try {
        $arguments = @('publish', '--access', 'public')
        if ($DryRun) { $arguments += '--dry-run' }
        & npm @arguments
        if ($LASTEXITCODE -ne 0) { throw "npm publish failed for $package" }
    }
    finally {
        Pop-Location
    }
    Write-Host "  ✓ Published $package"
}

Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination (Join-Path $npmDir 'clawmacdo/README.md') -Force

Write-Host '-> Publishing clawmacdo...'
Push-Location (Join-Path $npmDir 'clawmacdo')
try {
    $arguments = @('publish', '--access', 'public')
    if ($DryRun) { $arguments += '--dry-run' }
    & npm @arguments
    if ($LASTEXITCODE -ne 0) { throw 'npm publish failed for clawmacdo' }
}
finally {
    Pop-Location
}

Write-Host '  ✓ Published clawmacdo'
Write-Host ''
Write-Host 'Done! All packages published.'
Write-Host 'Install with: npm install -g clawmacdo'