#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [ValidateSet('patch', 'minor', 'major')]
    [string]$BumpType = 'minor'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$cargoToml = Join-Path $root 'crates/clawmacdo-cli/Cargo.toml'
$cargoContent = Get-Content -LiteralPath $cargoToml -Raw
$match = [regex]::Match($cargoContent, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $match.Success) {
    throw 'Could not determine current version from crates/clawmacdo-cli/Cargo.toml.'
}

$currentVersion = $match.Groups[1].Value
$parts = $currentVersion.Split('.')
[int]$major = $parts[0]
[int]$minor = $parts[1]
[int]$patch = $parts[2]

switch ($BumpType) {
    'patch' { $patch += 1 }
    'minor' { $minor += 1; $patch = 0 }
    'major' { $major += 1; $minor = 0; $patch = 0 }
}

$newVersion = "$major.$minor.$patch"
Write-Host "Bumping: $currentVersion -> $newVersion ($BumpType)"

function Invoke-Checked {
    param([string[]]$Command)

    & $Command[0] $Command[1..($Command.Length - 1)]
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed: $($Command -join ' ')"
    }
}

Write-Host '==> Formatting...'
Invoke-Checked @('cargo', 'fmt', '--all')

Write-Host '==> Linting...'
Invoke-Checked @('cargo', 'clippy', '--', '-D', 'warnings')

Write-Host "==> Bumping versions to $newVersion..."
foreach ($file in Get-ChildItem -LiteralPath (Join-Path $root 'crates') -Filter Cargo.toml -Recurse) {
    $content = Get-Content -LiteralPath $file.FullName -Raw
    $updated = [regex]::Replace($content, '(?m)^version\s*=\s*"' + [regex]::Escape($currentVersion) + '"', 'version = "' + $newVersion + '"', 1)
    Set-Content -LiteralPath $file.FullName -Value $updated -Encoding utf8
}

foreach ($packageJsonPath in @(
    (Join-Path $root 'npm/clawmacdo/package.json'),
    (Join-Path $root 'npm/@clawmacdo/darwin-arm64/package.json'),
    (Join-Path $root 'npm/@clawmacdo/linux-x64/package.json'),
    (Join-Path $root 'npm/@clawmacdo/win32-x64/package.json')
)) {
    $json = Get-Content -LiteralPath $packageJsonPath -Raw | ConvertFrom-Json
    $json.version = $newVersion
    foreach ($prop in @('optionalDependencies', 'dependencies')) {
        $property = $json.PSObject.Properties[$prop]
        if ($null -eq $property) { continue }
        foreach ($name in @($property.Value.PSObject.Properties.Name)) {
            if ($name -like '@clawmacdo/*') {
                $property.Value.$name = $newVersion
            }
        }
    }
    Set-Content -LiteralPath $packageJsonPath -Value ($json | ConvertTo-Json -Depth 20) -Encoding utf8
}

Write-Host '==> Updating CHANGELOG and README...'
$changelogPath = Join-Path $root 'CHANGELOG.md'
$changelog = Get-Content -LiteralPath $changelogPath -Raw
$changelog = $changelog -replace ('(?m)^## v' + [regex]::Escape($currentVersion) + '\b'), "## v$newVersion"
Set-Content -LiteralPath $changelogPath -Value $changelog -Encoding utf8

$readmePath = Join-Path $root 'README.md'
$readme = Get-Content -LiteralPath $readmePath -Raw
$readme = $readme -replace ('What''s New in v' + [regex]::Escape($currentVersion)), "What's New in v$newVersion"
if ($readme -match 'Current version:') {
    $readme = $readme -replace 'Current version:.*', "Current version:** $newVersion"
}
Set-Content -LiteralPath $readmePath -Value $readme -Encoding utf8

Write-Host '==> Syncing README to npm...'
Copy-Item -LiteralPath $readmePath -Destination (Join-Path $root 'npm/clawmacdo/README.md') -Force

Write-Host '==> Building release...'
Invoke-Checked @('cargo', 'build', '--release')

Write-Host '==> Committing and tagging...'
Invoke-Checked @('git', 'add', '-A')
Invoke-Checked @('git', 'commit', '-m', "chore: release v$newVersion")
Invoke-Checked @('git', 'tag', "v$newVersion")
Invoke-Checked @('git', 'push', 'origin', 'main')
Invoke-Checked @('git', 'push', 'origin', "v$newVersion")

Write-Host '==> Creating GitHub release...'
Invoke-Checked @(
    'gh', 'release', 'create', "v$newVersion",
    '--title', "v$newVersion",
    '--notes', "Release v$newVersion. See [CHANGELOG.md](https://github.com/kenken64/clawmacdo/blob/main/CHANGELOG.md) for details."
)

Write-Host ''
Write-Host "=== Release v$newVersion published ==="
Write-Host "  Release: https://github.com/kenken64/clawmacdo/releases/tag/v$newVersion"
Write-Host '  npm-publish and release workflows triggered.'