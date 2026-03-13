#!/usr/bin/env bash
# Usage: format_outputs.sh <json-in> <format> <out>
set -euo pipefail
json_in=${1:?}
format=${2:?}
out=${3:-${json_in}}
case "$format" in
  json) cp "$json_in" "$out" ;;
  text) # extract a short summary
    jq -r '"Host: " + (.host // "combined") + "\nChecks:\n" + ( .checks // [] | map("- " + (.id//"<id>") + ": " + (.value|tostring)) | join("\n") )' "$json_in" > "$out" ;;
  sarif) # minimal SARIF wrapper
    cat > "$out" <<SV
{ "schema": "https://schemastore.azurewebsites.net/schemas/json/sarif-2.1.0.json", "version": "2.1.0", "runs": [{"tool": {"driver": {"name":"OpenClawScanner"}}, "results": []}] }
SV
    ;;
  *) echo "Unknown format: $format"; exit 2;;
esac
