#!/usr/bin/env bash
# Install a local bundle with the real CLI into a home folder the build never used, with
# compilers and package downloads disabled, then exercise it and uninstall it.
set -euo pipefail
[ "${GITHUB_ACTIONS:-}" = true ] || { echo 'Run this on a clean GitHub Actions runner.' >&2; exit 1; }
repo=$(cd "$(dirname "$0")/../.." && pwd)
artifacts=${1:?Artifacts directory required}
platform=${2:?Platform required}
clawmacdo=${CLAWMACDO:?Set CLAWMACDO to the clawmacdo binary}
archive="$artifacts/hermes-$platform.tar.gz"
python=python3
command -v python3 >/dev/null 2>&1 || python=python

# The new home must not sit inside the build's home (/Users/runner, C:\Users\runneradmin),
# or leftover build paths would go unnoticed.
record_dir="$HOME/.clawmacdo"
case "$platform" in
  macos-arm64)
    sudo install -d -o "$(id -u)" -g "$(id -g)" /Users/clawmacdo-ci
    export HOME=/Users/clawmacdo-ci
    record_dir="$HOME/.clawmacdo"
    hermes_home="$HOME/.hermes"
    launcher="$HOME/.local/bin/hermes"
    ;;
  windows-x64)
    # clawmacdo keeps its record in the real profile; Hermes follows LOCALAPPDATA.
    mkdir -p "$RUNNER_TEMP/relocated/AppData/Local"
    USERPROFILE="$(cygpath -w "$RUNNER_TEMP/relocated")"
    LOCALAPPDATA="$(cygpath -w "$RUNNER_TEMP/relocated/AppData/Local")"
    export USERPROFILE LOCALAPPDATA
    hermes_home="$(cygpath -u "$LOCALAPPDATA")/hermes"
    launcher="$hermes_home/bin/hermes.exe"
    ;;
  *) echo "Unknown platform $platform" >&2; exit 1 ;;
esac

# Anything that tries to compile or download packages now fails instead of quietly
# rebuilding what the bundle should already contain.
export CC=false CXX=false npm_config_offline=true UV_OFFLINE=1
"$clawmacdo" hermes-local-install --bundle-archive "$archive" --no-service --no-opencode
[ -f "$launcher" ] || launcher="${launcher%.exe}.cmd"
test -f "$launcher"
grep -q '"install_method": "bundle"' "$record_dir/hermes-local.json"

"$python" "$repo/scripts/hermes-bundle/smoke_local.py" --launcher "$launcher" \
  --hermes-home "$hermes_home" --manifest "$archive.json"

"$clawmacdo" hermes-local-uninstall --yes
test ! -e "$hermes_home"
echo "Local bundle: relocated install, CLI, package doctor, dashboard and uninstall work"
