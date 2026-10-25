# Native bundle and OS setup are independent until service startup.
hermes_status() {
  local name="$1" message="$2"
  printf '%s\n' "$message" > "/var/lib/clawmacdo/hermes-progress/${name}.tmp"
  mv "/var/lib/clawmacdo/hermes-progress/${name}.tmp" "/var/lib/clawmacdo/hermes-progress/${name}.status"
  echo "[clawmacdo] $message"
}

hermes_os_setup() {
  local started=$SECONDS
  hermes_status os 'OS: refreshing package indexes'
  apt-get -o DPkg::Lock::Timeout=300 -o Acquire::Retries=3 -o Acquire::http::Timeout=60 update -y
  hermes_status os 'OS: upgrading installed packages'
  apt-get -o DPkg::Lock::Timeout=300 upgrade -y
  hermes_status os 'OS: installing runtime libraries'
  apt-get -o DPkg::Lock::Timeout=300 install -y ca-certificates curl git unzip python3 ufw fail2ban unattended-upgrades __RUNTIME_PACKAGES__
  hermes_status os "OS: ready in $((SECONDS - started))s"
}

hermes_bundle_install() {
  # This job only downloads and extracts files; it never uses apt/dpkg or starts Hermes.
  # Exit 42 means the release/platform is unavailable. Other errors fail provisioning.
  runuser -u ubuntu -- env HOME=/home/ubuntu python3 /tmp/clawmacdo-hermes-bundle.py \
    --config /tmp/clawmacdo-hermes-bundle.json
}

hermes_source_install() {
  hermes_status bundle 'Bundle unavailable: installing Hermes from pinned source'
  curl --fail --silent --show-error --location --retry 3 --connect-timeout 15 --max-time 180 \
    'https://raw.githubusercontent.com/NousResearch/hermes-agent/__HERMES_COMMIT__/scripts/install.sh' \
    -o /tmp/clawmacdo-hermes-install.sh
  printf '%s  %s\n' '__INSTALLER_SHA256__' /tmp/clawmacdo-hermes-install.sh | sha256sum -c -
  runuser -u ubuntu -- env HOME=/home/ubuntu HERMES_HOME=/home/ubuntu/.hermes \
    bash /tmp/clawmacdo-hermes-install.sh --non-interactive --commit '__HERMES_COMMIT__'
  hermes_status bundle 'Hermes source installation complete'
}

hermes_parallel_setup() {
  local os_pid bundle_pid os_rc=0 bundle_rc=0
  hermes_os_setup &
  os_pid=$!
  # An unavailable bundle is expected during rollout; handle its status after joining.
  (trap - ERR; hermes_bundle_install) &
  bundle_pid=$!
  wait "$os_pid" || os_rc=$?
  wait "$bundle_pid" || bundle_rc=$?
  # Always join both jobs. A failed OS update must never proceed to service startup.
  if [ "$os_rc" -ne 0 ]; then
    echo "[clawmacdo] OS setup failed (exit $os_rc)" >&2
    return "$os_rc"
  fi
  if [ "$bundle_rc" -eq 42 ]; then
    # The source installer may use apt, so it can run only after OS setup finishes.
    hermes_source_install
  elif [ "$bundle_rc" -ne 0 ]; then
    echo "[clawmacdo] Prebuilt bundle installation failed (exit $bundle_rc)" >&2
    return "$bundle_rc"
  fi
}

hermes_install_native() {
  install -d -m 755 -o ubuntu -g ubuntu /var/lib/clawmacdo/hermes-progress
  cat > /tmp/clawmacdo-hermes-bundle.json <<'BUNDLE_CONFIG'
__BUNDLE_CONFIG__
BUNDLE_CONFIG
  # Ubuntu 24.04 includes Python before apt starts; bootstrap uses only its stdlib.
  python3 - <<'BUNDLE_LOADER'
import base64, zlib
from pathlib import Path
Path('/tmp/clawmacdo-hermes-bundle.py').write_bytes(zlib.decompress(base64.b64decode('__BUNDLE_LOADER_ZLIB__')))
BUNDLE_LOADER
  chmod 644 /tmp/clawmacdo-hermes-bundle.json /tmp/clawmacdo-hermes-bundle.py
  echo '[clawmacdo] Installing native bundle and OS updates in parallel'
  hermes_parallel_setup
  test -x /home/ubuntu/.local/bin/hermes
}
