# Build the Windows x64 local bundle on a disposable GitHub Actions runner.
# Never package a user's profile.
param(
    [Parameter(Mandatory)][string]$Output,
    [Parameter(Mandatory)][string]$Report
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'Use the Hermes bundle workflow on a clean runner.' }
if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw "Expected an x64 runner, got $env:PROCESSOR_ARCHITECTURE" }
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$configPath = Join-Path $repo 'scripts/hermes-bundle/bundle.json'
$config = Get-Content $configPath -Raw | ConvertFrom-Json
$hermesHome = Join-Path $env:LOCALAPPDATA 'hermes'
if (Test-Path $hermesHome) { throw "$hermesHome already exists on this runner" }

$installer = Join-Path $env:RUNNER_TEMP 'hermes-bundle-installer.ps1'
Invoke-WebRequest -UseBasicParsing -OutFile $installer `
    "https://raw.githubusercontent.com/NousResearch/hermes-agent/$($config.hermes_commit)/scripts/install.ps1"
$hash = (Get-FileHash -Algorithm SHA256 $installer).Hash.ToLowerInvariant()
if ($hash -ne $config.installer_ps1_sha256) { throw "install.ps1 checksum mismatch: got $hash" }

$env:CI = '1'
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $installer -NonInteractive `
    -Commit $config.hermes_commit -SkipComputerUse
if ($LASTEXITCODE -ne 0) { throw "Hermes installer failed with exit code $LASTEXITCODE" }
$head = (git -C (Join-Path $hermesHome 'hermes-agent') rev-parse HEAD).Trim()
if ($head -ne $config.hermes_commit) { throw "Checkout is at $head, expected $($config.hermes_commit)" }

python (Join-Path $repo 'scripts/hermes-bundle/package.py') --config $configPath --output $Output `
    --platform windows-x64 --report $Report
if ($LASTEXITCODE -ne 0) { throw "Packaging failed with exit code $LASTEXITCODE" }
