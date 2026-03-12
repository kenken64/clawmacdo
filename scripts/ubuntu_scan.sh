#!/usr/bin/env bash
# Expanded Ubuntu read-only checks (MVP)
set -euo pipefail
out_file=${1:-/tmp/openclaw_ubuntu_scan_$(date +%s).json}
# helper
jq_empty='[]'
checks=$(mktemp)
# 1) sshd_config PermitRootLogin and PasswordAuthentication
sshd_conf=$(sed -n '1,400p' /etc/ssh/sshd_config 2>/dev/null || true)
permit_root=$(echo "$sshd_conf" | grep -Ei '^\s*PermitRootLogin' || echo 'not set')
password_auth=$(echo "$sshd_conf" | grep -Ei '^\s*PasswordAuthentication' || echo 'not set')
# 2) sudoers.d world-writable files
sudo_writable=$(find /etc/sudoers.d -maxdepth 1 -type f -perm /022 2>/dev/null || true)
# 3) world-writable /etc files
etc_writable=$(find /etc -maxdepth 2 -type f -perm /o+w 2>/dev/null | head -n 50 || true)
# 4) cron entries
cron_jobs=$(crontab -l 2>/dev/null || true)
sys_cron=$(ls -1 /etc/cron.* 2>/dev/null || true)
# 5) listening ports
listening=$(ss -ltnp 2>/dev/null || netstat -ltnp 2>/dev/null || true)
# 6) users with UID 0
u0=$(awk -F: '($3==0){print $1"("$3")"}' /etc/passwd || true)
# 7) unattended-upgrades status
unattended=$(systemctl is-enabled unattended-upgrades 2>/dev/null || echo "unknown")
# Build JSON
jq -n \
  --argpr sshd_pr "$permit_root" \
  --argpr sshd_pw "$password_auth" \
  --arg sudo_w "$sudo_writable" \
  --arg etc_w "$etc_writable" \
  --arg cron_u "$cron_jobs" \
  --arg cron_s "$sys_cron" \
  --arg listen "$listening" \
  --arg uid0 "$u0" \
  --arg unattended "$unattended" \
  '{host:"localhost",checks:[{id:"sshd_permit_root",value:$sshd_pr},{id:"sshd_password_auth",value:$sshd_pw},{id:"sudoers_world_writable",value:$sudo_w},{id:"etc_world_writable_sample",value:$etc_w},{id:"crontab_user",value:$cron_u},{id:"crontab_system_dirs",value:$cron_s},{id:"listening_ports",value:$listen},{id:"uid_0_accounts",value:$uid0},{id:"unattended_upgrades",value:$unattended}]}' > "$out_file"
rm -f "$checks"
echo "Wrote $out_file"
