#!/usr/bin/env bash
# Scan OpenClaw config and workspace for common issues (read-only)
set -euo pipefail
out_file=${1:-/tmp/openclaw_config_scan_$(date +%s).json}
base=/root/.openclaw
results=$(mktemp)
# 1) list important files
files=$(find $base -maxdepth 2 -type f -printf "%p\n" 2>/dev/null || true)
# 2) look for tokens/KEY-like patterns
secrets=$(grep -EIR --binary-files=without-match -n "(TOKEN|KEY|SECRET|PASSWORD|passwd|api_key|apiKey)" $base || true)
# 3) file permission issues (world-writable)
writable=$(find $base -type f -perm /o+w 2>/dev/null || true)
# 4) check known files: MEMORY.md, AGENTS.md, SOUL.md presence
has_memory=$( [ -f $base/workspace/MEMORY.md ] && echo yes || echo no )
# Build JSON
jq -n --arg files "$files" --arg secrets "$secrets" --arg writable "$writable" --arg memory "$has_memory" '{target:"openclaw_config",files:$files,secrets:$secrets,world_writable:$writable,has_MEMORY_md:$memory}' > "$out_file"
echo "Wrote $out_file"
