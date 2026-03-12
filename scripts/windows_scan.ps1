Param(
    [string]$OutPath = "C:\\Windows\\Temp\\openclaw_windows_scan.json"
)
# Basic Windows read-only scan (PowerShell)
$report = [ordered]@{}
$report.host = $env:COMPUTERNAME
# RDP status
try { $rdp = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server' -Name fDenyTSConnections -ErrorAction Stop; $report.rdp_enabled = ($rdp.fDenyTSConnections -eq 0) } catch { $report.rdp_enabled = 'unknown' }
# Windows Update status
try { $wu = Get-Service -Name wuauserv -ErrorAction Stop; $report.windows_update = $wu.Status } catch { $report.windows_update = 'unknown' }
# Firewall status
try { $fw = (Get-NetFirewallProfile | Select-Object Name,Enabled) ; $report.firewall = $fw } catch { $report.firewall = 'unknown' }
# Admin users
try { $admins = Get-LocalGroupMember -Group Administrators | Select-Object Name,ObjectClass ; $report.admins = $admins } catch { $report.admins = 'unknown' }
# Scheduled tasks (recent)
try { $tasks = Get-ScheduledTask | Select-Object TaskName,State | Select-Object -First 50; $report.scheduled_tasks = $tasks } catch { $report.scheduled_tasks = 'unknown' }
# Save as JSON
$report | ConvertTo-Json -Depth 5 | Out-File -FilePath $OutPath -Encoding utf8
Write-Output "Wrote $OutPath"
