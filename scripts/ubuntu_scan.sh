#!/usr/bin/env bash
# Lightweight read-only Ubuntu checks (MVP)
set -euo pipefail
out_file=${1:-/tmp/openclaw_ubuntu_scan_$(date +%s).json}
json='{"host":"localhost","checks":[]}'
# example check: sshd config
sshd_conf=$(sed -n '1,200p' /etc/ssh/sshd_config 2>/dev/null || true)
permit_root=$(echo "$sshd_conf" | grep -Ei '^\s*PermitRootLogin' || true)
printf '{"check":"sshd_permit_root","value":"%s"}\n' "$permit_root" > /tmp/_sshcheck.json
jq -n --arg host "localhost" --slurpfile c /tmp/_sshcheck.json '{host:$host,checks:$c}' > "$out_file" || true
echo "Wrote $out_file"
