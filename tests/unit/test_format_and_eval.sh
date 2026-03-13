#!/usr/bin/env bash
set -euo pipefail
# format to text
./scripts/format_outputs.sh fixtures/sample_high_risk.json text /tmp/sample_text.txt
grep -q "sshd_permit_root" /tmp/sample_text.txt
# eval risk
if ./scripts/evaluate_risk.sh fixtures/sample_high_risk.json; then
  echo "FAIL: expected non-zero exit"; exit 2
else
  echo "PASS: evaluator returned non-zero for high risk"
fi
