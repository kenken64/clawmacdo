#!/usr/bin/env pwsh
[CmdletBinding()]
param(
    [string]$OutFile = (Join-Path ([System.IO.Path]::GetTempPath()) ("openclaw_ubuntu_scan_{0}.json" -f [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()))
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-ExternalOutput {
    param(
        [string]$Command,
        [string[]]$Arguments = @(),
        [string]$Fallback = ''
    )

    try {
        $output = & $Command @Arguments 2>$null | Out-String
        $trimmed = $output.Trim()
        if ([string]::IsNullOrWhiteSpace($trimmed)) { return $Fallback }
        return $trimmed
    }
    catch {
        return $Fallback
    }
}

$sshdConf = if (Test-Path '/etc/ssh/sshd_config') { Get-Content '/etc/ssh/sshd_config' -Raw -ErrorAction SilentlyContinue } else { '' }
$permitRootMatch = [regex]::Match($sshdConf, '(?m)^\s*PermitRootLogin.*$')
$passwordAuthMatch = [regex]::Match($sshdConf, '(?m)^\s*PasswordAuthentication.*$')
$permitRoot = if ($permitRootMatch.Success) { $permitRootMatch.Value.Trim() } else { 'not set' }
$passwordAuth = if ($passwordAuthMatch.Success) { $passwordAuthMatch.Value.Trim() } else { 'not set' }

$sudoWritable = Get-ExternalOutput -Command 'find' -Arguments @('/etc/sudoers.d', '-maxdepth', '1', '-type', 'f', '-perm', '/022')
$etcWritable = Get-ExternalOutput -Command 'find' -Arguments @('/etc', '-maxdepth', '2', '-type', 'f', '-perm', '/o+w')
$cronJobs = Get-ExternalOutput -Command 'crontab' -Arguments @('-l')
$systemCron = Get-ExternalOutput -Command 'bash' -Arguments @('-lc', 'ls -1 /etc/cron.* 2>/dev/null')

$listening = Get-ExternalOutput -Command 'ss' -Arguments @('-ltnp')
if ([string]::IsNullOrWhiteSpace($listening)) {
    $listening = Get-ExternalOutput -Command 'netstat' -Arguments @('-ltnp')
}

$uid0Accounts = @()
if (Test-Path '/etc/passwd') {
    foreach ($line in Get-Content '/etc/passwd') {
        $parts = $line.Split(':')
        if ($parts.Length -ge 3 -and $parts[2] -eq '0') {
            $uid0Accounts += "{0}({1})" -f $parts[0], $parts[2]
        }
    }
}

$unattended = Get-ExternalOutput -Command 'systemctl' -Arguments @('is-enabled', 'unattended-upgrades') -Fallback 'unknown'

$report = [ordered]@{
    host   = 'localhost'
    checks = @(
        [ordered]@{ id = 'sshd_permit_root'; value = $permitRoot },
        [ordered]@{ id = 'sshd_password_auth'; value = $passwordAuth },
        [ordered]@{ id = 'sudoers_world_writable'; value = $sudoWritable },
        [ordered]@{ id = 'etc_world_writable_sample'; value = $etcWritable },
        [ordered]@{ id = 'crontab_user'; value = $cronJobs },
        [ordered]@{ id = 'crontab_system_dirs'; value = $systemCron },
        [ordered]@{ id = 'listening_ports'; value = $listening },
        [ordered]@{ id = 'uid_0_accounts'; value = ($uid0Accounts -join [Environment]::NewLine) },
        [ordered]@{ id = 'unattended_upgrades'; value = $unattended }
    )
}

Set-Content -LiteralPath $OutFile -Value ($report | ConvertTo-Json -Depth 8) -Encoding utf8
Write-Host "Wrote $OutFile"