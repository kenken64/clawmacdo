#!/usr/bin/env bash
# Very small heuristic risk evaluator
set -euo pipefail
json=${1:?}
# Defaults
risk=0
notes="[]"
# check for api keys in openclaw config scan
if jq -e '.secrets | length>0' "$json" >/dev/null 2>&1; then
  risk=2
  notes=$(jq -r '.secrets' "$json" | sed 's/"/\\"/g' | jq -R -s '.' )
fi
# check for sshd permit root
if jq -e '.checks?[]?.id' "$json" >/dev/null 2>&1; then
  pr=$(jq -r '.checks[]? | select(.id=="sshd_permit_root") | .value' "$json" 2>/dev/null || echo "")
  if [ "$pr" = "PermitRootLogin yes" ]; then risk=2; fi
fi
# Emit structured log to stdout
jq -n --arg time "$(date -u -Iseconds)" --argjson notes "$notes" --arg risk "$risk" '{time:$time,kind:"scan_evaluation",risk:(($risk|tonumber)),notes:$notes}'
exit $risk
