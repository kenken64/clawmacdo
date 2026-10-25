#!/usr/bin/env bash
# Started by systemd only after the client saves the completed deployment.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
# List services needing a restart; do not ask needrestart to interrupt Hermes.
export NEEDRESTART_MODE=l
STATE_DIR=/var/lib/clawmacdo/hermes-os-upgrade
install -d -m 755 "$STATE_DIR"

report() {
  printf '%s\n' "$1" > "$STATE_DIR/status.tmp"
  mv "$STATE_DIR/status.tmp" "$STATE_DIR/status"
  echo "[clawmacdo] OS upgrade: $1"
}

finish() {
  local rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    report "failed (exit $rc); see journalctl -u clawmacdo-hermes-os-upgrade"
  fi
  exit "$rc"
}
trap finish EXIT

report running
apt-get -o DPkg::Lock::Timeout=300 -o Acquire::Retries=3 -o Acquire::http::Timeout=60 \
  -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold upgrade -y

if [ -f /var/run/reboot-required ]; then
  report 'completed; reboot required (not performed automatically)'
else
  report completed
fi
