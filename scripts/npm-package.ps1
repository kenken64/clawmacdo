#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [switch]$Local
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$npmDir = Join-Path $root 'npm'

function Get-CargoVersion {
    $cargoToml = Join-Path $root 'crates/clawmacdo-cli/Cargo.toml'
    $match = [regex]::Match((Get-Content -LiteralPath $cargoToml -Raw), '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $match.Success) {
        throw 'Could not determine version from clawmacdo-cli Cargo.toml.'
    }
    $match.Groups[1].Value
}

function Update-PackageVersion {
    param([string]$PackageJsonPath, [string]$Version)

    $json = Get-Content -LiteralPath $PackageJsonPath -Raw | ConvertFrom-Json
    $json.version = $Version

    foreach ($propertyName in @('optionalDependencies', 'dependencies')) {
        $property = $json.PSObject.Properties[$propertyName]
        if ($null -eq $property) { continue }
        foreach ($dependency in @($property.Value.PSObject.Properties.Name)) {
            if ($dependency -like '@clawmacdo/*') {
                $property.Value.$dependency = $Version
            }
        }
    }

    Set-Content -LiteralPath $PackageJsonPath -Value ($json | ConvertTo-Json -Depth 20) -Encoding utf8
}

function Resolve-TargetInfo {
    param([string]$Target)

    switch ($Target) {
        'x86_64-unknown-linux-gnu' { return @{ PlatformDir = 'linux-x64'; BinaryName = 'clawmacdo' } }
        'aarch64-apple-darwin' { return @{ PlatformDir = 'darwin-arm64'; BinaryName = 'clawmacdo' } }
        'x86_64-pc-windows-gnu' { return @{ PlatformDir = 'win32-x64'; BinaryName = 'clawmacdo.exe' } }
        'x86_64-pc-windows-msvc' { return @{ PlatformDir = 'win32-x64'; BinaryName = 'clawmacdo.exe' } }
        default { throw "Unknown target: $Target" }
    }
}

function Build-Target {
    param([string]$Target)

    $targetInfo = Resolve-TargetInfo -Target $Target
    $destDir = Join-Path $npmDir "@clawmacdo/$($targetInfo.PlatformDir)/bin"

    Write-Host "-> Building for $Target ($($targetInfo.PlatformDir))..."
    cargo build --release --target $Target
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $Target" }

    New-Item -ItemType Directory -Path $destDir -Force | Out-Null
    $binaryPath = Join-Path $root "target/$Target/release/$($targetInfo.BinaryName)"
    Copy-Item -LiteralPath $binaryPath -Destination (Join-Path $destDir $targetInfo.BinaryName) -Force

    if (-not $IsWindows) {
        & chmod +x (Join-Path $destDir $targetInfo.BinaryName)
    }

    Write-Host "  ✓ Copied to $(Join-Path $destDir $targetInfo.BinaryName)"
}

$version = Get-CargoVersion
Write-Host "Building clawmacdo v$version for npm distribution"

foreach ($packageJson in @(
    (Join-Path $npmDir 'clawmacdo/package.json'),
    (Join-Path $npmDir '@clawmacdo/darwin-arm64/package.json'),
    (Join-Path $npmDir '@clawmacdo/linux-x64/package.json'),
    (Join-Path $npmDir '@clawmacdo/win32-x64/package.json')
)) {
    if (Test-Path -LiteralPath $packageJson) {
        Update-PackageVersion -PackageJsonPath $packageJson -Version $version
    }
}

Write-Host "  ✓ All package versions set to $version"

$targets = @('x86_64-unknown-linux-gnu', 'aarch64-apple-darwin', 'x86_64-pc-windows-gnu')
if ($Local) {
    if ($IsMacOS -and [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') {
        Build-Target -Target 'aarch64-apple-darwin'
    }
    elseif ($IsLinux -and [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'X64') {
        Build-Target -Target 'x86_64-unknown-linux-gnu'
    }
    elseif ($IsWindows -and [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'X64') {
        Build-Target -Target 'x86_64-pc-windows-msvc'
    }
    else {
        throw "Unsupported local platform: $([System.Runtime.InteropServices.RuntimeInformation]::OSDescription) / $([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture)"
    }
}
else {
    foreach ($target in $targets) {
        Build-Target -Target $target
    }
}

Write-Host ''
Write-Host "Done! Packages ready in $npmDir"
Write-Host 'To publish, run: ./scripts/npm-publish.ps1'