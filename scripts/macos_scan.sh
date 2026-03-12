#!/usr/bin/env bash
# Basic macOS read-only checks (MVP)
set -euo pipefail
out_file=${1:-/tmp/openclaw_macos_scan_$(date +%s).json}
# Gather SIP status
sip_stat=$(csrutil status 2>/dev/null || echo "csrutil not available")
# Gatekeeper
gatekeeper=$(spctl --status 2>/dev/null || echo "spctl not available")
jq -n --arg sip "$sip_stat" --arg gk "$gatekeeper" '{host:"localhost-macos",checks:[{check:"sip_status",value:$sip},{check:"gatekeeper",value:$gk}]}' > "$out_file"
echo "Wrote $out_file"
