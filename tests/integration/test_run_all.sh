#!/usr/bin/env bash
set -euo pipefail
./scripts/run_all_scans.sh
# find latest JSON
json=$(ls -1 /tmp/openclaw_security_scan_*.json | tail -n1)
if [ -z "$json" ]; then
  echo "FAIL: no json produced"; exit 2
fi
# quick schema check: must contain "parts"
grep -q '"parts"' "$json" || (echo "FAIL: parts not found in $json"; exit 3)
echo "PASS: $json contains parts"
