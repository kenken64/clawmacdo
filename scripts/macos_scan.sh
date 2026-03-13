#!/usr/bin/env bash
# Expanded macOS read-only checks (MVP)
set -euo pipefail
out_file=${1:-/tmp/openclaw_macos_scan_$(date +%s).json}
# SIP
sip_stat=$(csrutil status 2>/dev/null || echo "csrutil not available")
# Gatekeeper
gatekeeper=$(spctl --status 2>/dev/null || echo "spctl not available")
# FileVault status
fv=$(fdesetup status 2>/dev/null || echo "fdesetup not available")
# LaunchDaemons and LaunchAgents sample
ld=$(ls /Library/LaunchDaemons 2>/dev/null | head -n 20 || true)
la=$(ls /Library/LaunchAgents 2>/dev/null | head -n 20 || true)
# Firewall
fw=$(defaults read /Library/Preferences/com.apple.alf globalstate 2>/dev/null || echo "firewall status unknown")
jq -n \
  --arg sip "$sip_stat" \
  --arg gk "$gatekeeper" \
  --arg fv "$fv" \
  --arg ld "$ld" \
  --arg la "$la" \
  --arg fw "$fw" \
  '{host:"localhost-macos",checks:[{id:"sip_status",value:$sip},{id:"gatekeeper",value:$gk},{id:"filevault",value:$fv},{id:"launch_daemons_sample",value:$ld},{id:"launch_agents_sample",value:$la},{id:"firewall",value:$fw}]}' > "$out_file"

echo "Wrote $out_file"
