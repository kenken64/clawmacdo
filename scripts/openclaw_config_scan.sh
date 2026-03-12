#!/usr/bin/env bash
# Scan OpenClaw config and workspace for common issues (read-only)
set -euo pipefail
out_file=${1:-/tmp/openclaw_config_scan_$(date +%s).json}
base=/root/.openclaw
# 1) list important files
files=$(find "$base" -maxdepth 2 -type f -printf "%p\n" 2>/dev/null || true)
# 2) look for tokens/KEY-like patterns
secrets=$(grep -EIR --binary-files=without-match -n "(TOKEN|KEY|SECRET|PASSWORD|passwd|api_key|apiKey)" "$base" || true)
# 3) file permission issues (world-writable)
writable=$(find "$base" -type f -perm /o+w 2>/dev/null || true)
# 4) check known files: MEMORY.md, AGENTS.md, SOUL.md presence
has_memory=$( [ -f "$base/workspace/MEMORY.md" ] && echo yes || echo no )
# write temp files and use jq --rawfile to avoid huge args
tmpf_files=$(mktemp)
tmpf_secrets=$(mktemp)
tmpf_writable=$(mktemp)
printf '%s' "$files" > "$tmpf_files"
printf '%s' "$secrets" > "$tmpf_secrets"
printf '%s' "$writable" > "$tmpf_writable"
jq -n --rawfile files "$tmpf_files" --rawfile secrets "$tmpf_secrets" --rawfile writable "$tmpf_writable" --arg memory "$has_memory" '{target:"openclaw_config",files:$files,secrets:$secrets,world_writable:$writable,has_MEMORY_md:$memory}' > "$out_file"
rm -f "$tmpf_files" "$tmpf_secrets" "$tmpf_writable"
echo "Wrote $out_file"
