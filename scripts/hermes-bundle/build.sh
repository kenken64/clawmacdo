#!/usr/bin/env bash
# Build only on a disposable Ubuntu GitHub Actions VM. Never package a user's profile.
set -euo pipefail
[ "${GITHUB_ACTIONS:-}" = true ] || { echo 'Use the Hermes bundle workflow on a clean runner.' >&2; exit 1; }
repo=$(cd "$(dirname "$0")/../.." && pwd)
config="$repo/scripts/hermes-bundle/bundle.json"
out=${1:?Output directory required}
arch=$(dpkg --print-architecture)
. /etc/os-release
[ "$ID:$VERSION_ID" = ubuntu:24.04 ]
test ! -e /home/ubuntu/.hermes
read_config() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$config" "$1"; }
commit=$(read_config hermes_commit)

sudo apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=60 update
mapfile -t runtime_packages < <(python3 -c 'import json,sys; print("\n".join(json.load(open(sys.argv[1]))["runtime_packages"]))' "$config")
sudo apt-get install -y ca-certificates curl git unzip python3 "${runtime_packages[@]}"
id ubuntu >/dev/null 2>&1 || sudo useradd --create-home --shell /bin/bash ubuntu
sudo install -d -o ubuntu -g ubuntu /home/ubuntu

curl -fSL --retry 3 "https://raw.githubusercontent.com/NousResearch/hermes-agent/$commit/scripts/install.sh" -o /tmp/hermes-bundle-installer.sh
printf '%s  %s\n' "$(read_config installer_sha256)" /tmp/hermes-bundle-installer.sh | sha256sum -c -
# The source installer prepares its own pinned Python, tool store and dependency generations.
sudo runuser -u ubuntu -- env -i HOME=/home/ubuntu USER=ubuntu LOGNAME=ubuntu \
  PATH=/usr/local/bin:/usr/bin:/bin HERMES_HOME=/home/ubuntu/.hermes CI=1 \
  DEBIAN_FRONTEND=noninteractive NEEDRESTART_MODE=a \
  bash /tmp/hermes-bundle-installer.sh --non-interactive --commit "$commit"
test "$(sudo -u ubuntu git -C /home/ubuntu/.hermes/hermes-agent rev-parse HEAD)" = "$commit"

curl -fSL --retry 3 https://opencode.ai/v2/install -o /tmp/hermes-bundle-opencode.sh
printf '%s  %s\n' "$(read_config opencode_installer_sha256)" /tmp/hermes-bundle-opencode.sh | sha256sum -c -
sudo runuser -u ubuntu -- env -i HOME=/home/ubuntu PATH=/usr/local/bin:/usr/bin:/bin \
  bash /tmp/hermes-bundle-opencode.sh --version "$(read_config opencode_version)" --no-modify-path

sudo install -d -o ubuntu -g ubuntu /home/ubuntu/.local/lib/clawmacdo
curl -fSL --retry 3 "https://github.com/cloudflare/cloudflared/releases/download/$(read_config cloudflared_version)/cloudflared-linux-$arch" -o /tmp/hermes-bundle-cloudflared
cf_hash=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["cloudflared_sha256"][sys.argv[2]])' "$config" "$arch")
printf '%s  %s\n' "$cf_hash" /tmp/hermes-bundle-cloudflared | sha256sum -c -
sudo install -o ubuntu -g ubuntu -m 755 /tmp/hermes-bundle-cloudflared /home/ubuntu/.local/lib/clawmacdo/cloudflared

mkdir -p "$out"
sudo --preserve-env=GITHUB_ACTIONS python3 "$repo/scripts/hermes-bundle/package.py" --config "$config" --output "$out" --arch "$arch"
sudo chown -R "$(id -u):$(id -g)" "$out"
