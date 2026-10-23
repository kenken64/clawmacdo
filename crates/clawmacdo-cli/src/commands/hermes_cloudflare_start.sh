echo "[clawmacdo] Starting the Hermes dashboard Quick Tunnel"
systemctl daemon-reload
# ExecStartPost discovers the URL and checks Hermes auth on every connector start.
systemctl enable --now hermes-cloudflared.service
systemctl is-active --quiet hermes-cloudflared.service
test -s /var/lib/hermes-cloudflared/dashboard-url
