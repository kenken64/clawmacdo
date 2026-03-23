#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [string]$OutFile = (Join-Path ([System.IO.Path]::GetTempPath()) ("openclaw_macos_scan_{0}.json" -f [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()))
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-CommandText {
    param(
        [scriptblock]$Script,
        [string]$Fallback
    )

    try {
        $output = & $Script 2>$null | Out-String
        $trimmed = $output.Trim()
        if ([string]::IsNullOrWhiteSpace($trimmed)) { return $Fallback }
        return $trimmed
    }
    catch {
        return $Fallback
    }
}

$sipStatus = Get-CommandText -Script { csrutil status } -Fallback 'csrutil not available'
$gatekeeper = Get-CommandText -Script { spctl --status } -Fallback 'spctl not available'
$fileVault = Get-CommandText -Script { fdesetup status } -Fallback 'fdesetup not available'
$launchDaemons = if (Test-Path '/Library/LaunchDaemons') { ((Get-ChildItem '/Library/LaunchDaemons' -ErrorAction SilentlyContinue | Select-Object -First 20 -ExpandProperty Name) -join [Environment]::NewLine) } else { '' }
$launchAgents = if (Test-Path '/Library/LaunchAgents') { ((Get-ChildItem '/Library/LaunchAgents' -ErrorAction SilentlyContinue | Select-Object -First 20 -ExpandProperty Name) -join [Environment]::NewLine) } else { '' }
$firewall = Get-CommandText -Script { defaults read /Library/Preferences/com.apple.alf globalstate } -Fallback 'firewall status unknown'

$report = [ordered]@{
    host   = 'localhost-macos'
    checks = @(
        [ordered]@{ id = 'sip_status'; value = $sipStatus },
        [ordered]@{ id = 'gatekeeper'; value = $gatekeeper },
        [ordered]@{ id = 'filevault'; value = $fileVault },
        [ordered]@{ id = 'launch_daemons_sample'; value = $launchDaemons },
        [ordered]@{ id = 'launch_agents_sample'; value = $launchAgents },
        [ordered]@{ id = 'firewall'; value = $firewall }
    )
}

Set-Content -LiteralPath $OutFile -Value ($report | ConvertTo-Json -Depth 8) -Encoding utf8
Write-Host "Wrote $OutFile"