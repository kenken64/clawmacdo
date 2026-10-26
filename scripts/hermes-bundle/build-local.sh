#!/usr/bin/env bash
# Build the macOS Apple Silicon local bundle on a disposable GitHub Actions runner.
# Never package a user's profile.
set -euo pipefail
[ "${GITHUB_ACTIONS:-}" = true ] || { echo 'Use the Hermes bundle workflow on a clean runner.' >&2; exit 1; }
repo=$(cd "$(dirname "$0")/../.." && pwd)
config="$repo/scripts/hermes-bundle/bundle.json"
out=${1:?Output directory required}
report=${2:?Report path required}
[ "$(uname -s):$(uname -m)" = Darwin:arm64 ]
test ! -e "$HOME/.hermes"
read_config() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$config" "$1"; }
commit=$(read_config hermes_commit)

installer="$RUNNER_TEMP/hermes-bundle-installer.sh"
curl -fSL --retry 3 "https://raw.githubusercontent.com/NousResearch/hermes-agent/$commit/scripts/install.sh" -o "$installer"
printf '%s  %s\n' "$(read_config installer_sha256)" "$installer" | shasum -a 256 -c -
# The installer stages its own pinned Python, Node, npm and uv in the Hermes tool store.
# A minimal PATH keeps runner tools out of the build: with all of Homebrew visible, the
# installer builds its runtime on Homebrew's Python, which users do not have. Only
# Homebrew's git is exposed, because Apple's git fails upstream's partial-clone checkout
# ("expected 'packfile'").
git_only="$RUNNER_TEMP/git-only-bin"
mkdir -p "$git_only"
ln -sf /opt/homebrew/bin/git "$git_only/git"
env -i HOME="$HOME" USER="$USER" LOGNAME="$USER" PATH="$git_only:/usr/bin:/bin:/usr/sbin:/sbin" \
  TMPDIR="$RUNNER_TEMP" LANG=en_US.UTF-8 CI=1 \
  bash "$installer" --non-interactive --commit "$commit" --skip-computer-use
test "$(git -C "$HOME/.hermes/hermes-agent" rev-parse HEAD)" = "$commit"

python3 "$repo/scripts/hermes-bundle/package.py" --config "$config" --output "$out" \
  --platform macos-arm64 --report "$report"
