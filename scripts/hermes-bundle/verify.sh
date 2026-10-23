#!/usr/bin/env bash
# Run on a second, fresh VM: the builder's filesystem/caches must not be available.
set -euo pipefail
[ "${GITHUB_ACTIONS:-}" = true ]
repo=$(cd "$(dirname "$0")/../.." && pwd)
artifacts=${1:?Artifacts directory required}
arch=$(dpkg --print-architecture)
test ! -e /home/ubuntu/.hermes
mapfile -t packages < <(python3 -c 'import json,sys; print("\n".join(json.load(open(sys.argv[1]))["runtime_packages"]))' "$repo/scripts/hermes-bundle/bundle.json")
apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=60 update
apt-get install -y "${packages[@]}"
id ubuntu >/dev/null 2>&1 || useradd --create-home --shell /bin/bash ubuntu
install -d -o ubuntu -g ubuntu /home/ubuntu
# GitHub's runner temp parent is private; copy only these public artifacts to a readable path.
install -d -m 755 /tmp/hermes-bundle-verify
cp "$artifacts"/* /tmp/hermes-bundle-verify/
cp "$repo/crates/clawmacdo-cli/src/commands/hermes_bundle.py" "$repo/scripts/hermes-bundle/bundle.json" "$repo/scripts/hermes-bundle/smoke.py" /tmp/hermes-bundle-verify/
chmod 644 /tmp/hermes-bundle-verify/*
runuser -u ubuntu -- python3 /tmp/hermes-bundle-verify/hermes_bundle.py \
  --config /tmp/hermes-bundle-verify/bundle.json \
  --archive "/tmp/hermes-bundle-verify/hermes-ubuntu-24.04-$arch.tar.gz"
# Offline processes cannot hide missing dependencies by re-running an installer.
unshare --net bash -c 'ip link set lo up; exec runuser -u ubuntu -- env -i HOME=/home/ubuntu USER=ubuntu PATH=/usr/bin:/bin HERMES_HOME=/home/ubuntu/.hermes UV_OFFLINE=1 npm_config_offline=true python3 "$1"' \
  bash /tmp/hermes-bundle-verify/smoke.py
