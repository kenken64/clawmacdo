#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd "$(dirname "$0")" && pwd)"
pwsh -NoProfile -File "$script_dir/test_format_and_eval.ps1" "$@"
