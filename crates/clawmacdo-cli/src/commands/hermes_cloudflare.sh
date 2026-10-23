# Included in native Hermes cloud-init when a Quick Tunnel is requested.
echo "[clawmacdo] Installing cloudflared"
CF_ARCH=$(dpkg --print-architecture)
case "$CF_ARCH" in
  amd64|arm64) ;;
  *) echo "Unsupported cloudflared architecture: $CF_ARCH"; exit 1 ;;
esac
curl --fail --silent --show-error --location --retry 3 --connect-timeout 15 --max-time 180 \
  "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-${CF_ARCH}.deb" \
  -o /tmp/clawmacdo-cloudflared.deb
dpkg -i /tmp/clawmacdo-cloudflared.deb
rm -f /tmp/clawmacdo-cloudflared.deb
install -d -m 700 /etc/hermes-cloudflared
install -d -m 755 /var/lib/hermes-cloudflared /usr/local/lib/clawmacdo
printf '%s' '__TUNNEL_READY_B64__' | base64 -d > /usr/local/lib/clawmacdo/hermes-cloudflare-ready.py
chmod 644 /usr/local/lib/clawmacdo/hermes-cloudflare-ready.py

# Enable dashboard auth before cloudflared connects. The temporary public URL
# rejects external Host headers until the startup hook installs the real URL.
python3 - <<'PY'
import base64
import hashlib
import json
import secrets
from pathlib import Path

settings = json.loads(base64.b64decode('__TUNNEL_CONFIG_B64__'))
salt = secrets.token_bytes(16)
digest = hashlib.scrypt(settings['password'].encode(), salt=salt, n=16384, r=8, p=1, dklen=32)
password_hash = 'scrypt$16384$8$1$' + base64.b64encode(salt).decode() + '$' + base64.b64encode(digest).decode()
dashboard = {
    'public_url': 'https://pending.invalid',
    'basic_auth': {
        'username': 'admin',
        'password_hash': password_hash,
        'secret': secrets.token_hex(32),
    },
}
with open('/home/ubuntu/.hermes/config.yaml', 'a') as f:
    f.write('\ndashboard: ' + json.dumps(dashboard) + '\n')
env_path = Path('/etc/hermes-cloudflared/dashboard.env')
env_path.write_text('HERMES_DASHBOARD_PUBLIC_URL=https://pending.invalid\n')
env_path.chmod(0o600)
PY

cat > /etc/systemd/system/hermes-cloudflared.service <<'UNIT'
[Unit]
Description=Cloudflare Quick Tunnel for Hermes dashboard
# No ordering against hermes-dashboard: ExecStartPost restarts it after URL discovery.
After=network-online.target
Wants=network-online.target hermes-dashboard.service
StartLimitIntervalSec=0

[Service]
Type=notify
DynamicUser=yes
RuntimeDirectory=hermes-cloudflared
RuntimeDirectoryMode=0700
ExecStartPre=/usr/bin/truncate --size 0 /run/hermes-cloudflared/cloudflared.log
ExecStartPre=+/usr/bin/rm -f /var/lib/hermes-cloudflared/dashboard-url
ExecStart=/usr/bin/cloudflared tunnel --no-autoupdate --metrics 127.0.0.1:20242 --logfile /run/hermes-cloudflared/cloudflared.log --url http://127.0.0.1:9119
ExecStartPost=+/usr/bin/python3 /usr/local/lib/clawmacdo/hermes-cloudflare-ready.py
Restart=on-failure
RestartSec=5
TimeoutStartSec=300
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes

[Install]
WantedBy=multi-user.target
UNIT
