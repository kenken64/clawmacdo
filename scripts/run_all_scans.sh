#!/usr/bin/env bash
set -euo pipefail
ts=$(date +%s)
out=/tmp/openclaw_security_scan_${ts}.json
summary=/tmp/openclaw_security_scan_${ts}.summary.txt
# run component scans
./scripts/ubuntu_scan.sh /tmp/scan_ubuntu_${ts}.json || true
./scripts/macos_scan.sh /tmp/scan_macos_${ts}.json || true
./scripts/openclaw_config_scan.sh /tmp/openclaw_config_scan_${ts}.json || true
# combine
jq -s '{generated_at:(now|strftime("%Y-%m-%dT%H:%M:%SZ")),parts:.}' /tmp/scan_ubuntu_${ts}.json /tmp/scan_macos_${ts}.json /tmp/openclaw_config_scan_${ts}.json > ${out} || cp /tmp/scan_ubuntu_${ts}.json ${out} || true
# small human summary
echo "Security scan run at $(date -u -Iseconds)" > ${summary}
echo "Combined JSON: ${out}" >> ${summary}
echo "Components:" >> ${summary}
[ -f /tmp/scan_ubuntu_${ts}.json ] && echo "- Ubuntu: /tmp/scan_ubuntu_${ts}.json" >> ${summary} || true
[ -f /tmp/scan_macos_${ts}.json ] && echo "- macOS: /tmp/scan_macos_${ts}.json" >> ${summary} || true
[ -f /tmp/openclaw_config_scan_${ts}.json ] && echo "- OpenClaw config: /tmp/openclaw_config_scan_${ts}.json" >> ${summary} || true
# copy to workspace so messaging tool can reach it if needed
cp ${out} /root/.openclaw/workspace/openclaw_security_scan_${ts}.json || true
cp ${summary} /root/.openclaw/workspace/openclaw_security_scan_${ts}.summary.txt || true
echo "Wrote ${out} and summary ${summary}"
