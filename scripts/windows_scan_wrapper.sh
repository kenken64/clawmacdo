#!/usr/bin/env bash
# Wrapper: place windows_scan.ps1 in scripts/ and instruct user to run on Windows.
out_file=${1:-/tmp/openclaw_windows_scan_$(date +%s).json}
if [ -f scripts/windows_scan.ps1 ]; then
  echo "Windows scan script exists. Copy scripts/windows_scan.ps1 to the Windows host and run: pwsh ./windows_scan.ps1 -OutPath <path>"
  echo "Placeholder JSON" > "$out_file"
  echo "Wrote $out_file"
else
  echo "No windows_scan.ps1 present; create one or run the wrapper with a script" > "$out_file"
  echo "Wrote $out_file"
fi
