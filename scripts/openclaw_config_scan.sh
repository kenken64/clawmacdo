#!/usr/bin/env bash
# Targeted OpenClaw config scan (read-only, resource-friendly)
set -euo pipefail
out_file=${1:-/tmp/openclaw_config_scan_$(date +%s).json}
base=/root/.openclaw
files_list=""
secrets_list=""
writable_list=""
# Inspect key config files only
for f in "$base/openclaw.json" "$base/openclaw.json.bak" "$base/openclaw.json.bak.3" "$base/workspace/MEMORY.md" "$base/AGENTS.md" "$base/SOUL.md"; do
  [ -f "$f" ] && files_list+="$f\n"
  if [ -f "$f" ]; then
    if grep -Eiq "(TOKEN|KEY|SECRET|PASSWORD|passwd|api_key|apiKey)" "$f" 2>/dev/null; then
      secrets_list+="$f\n"
    fi
  fi
done
# check world-writable only in these dirs
for d in "$base" "$base/workspace"; do
  if [ -d "$d" ]; then
    writable_list+=$(find "$d" -maxdepth 2 -type f -perm /o+w 2>/dev/null || true)
  fi
done
has_memory=$( [ -f "$base/workspace/MEMORY.md" ] && echo yes || echo no )
# write small JSON
jq -n --arg files "$files_list" --arg secrets "$secrets_list" --arg writable "$writable_list" --arg memory "$has_memory" '{target:"openclaw_config",files:$files,secrets:$secrets,world_writable:$writable,has_MEMORY_md:$memory}' > "$out_file"
echo "Wrote $out_file"
