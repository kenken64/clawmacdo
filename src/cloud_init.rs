use crate::config::CLOUD_INIT_SENTINEL;

/// Generate a cloud-init YAML script for the droplet.
///
/// The script installs system packages, Node 24, OpenClaw, Claude Code, Codex,
/// configures UFW, writes API keys to /root/.openclaw/.env, creates a systemd
/// unit for the OpenClaw gateway, and touches a sentinel file on completion.
///
/// The systemd service is written but NOT started — deploy step 10 starts it
/// after config restore.
pub fn generate(anthropic_key: &str, openai_key: &str) -> String {
    format!(
        r##"#cloud-config
package_update: true
package_upgrade: true

packages:
  - curl
  - gnupg
  - ufw
  - git
  - build-essential

runcmd:
  # --- Firewall ---
  - ufw default deny incoming
  - ufw default allow outgoing
  - ufw allow 22/tcp
  - ufw allow 18789/tcp
  - ufw --force enable

  # --- Node.js 24 LTS via NodeSource ---
  - curl -fsSL https://deb.nodesource.com/setup_24.x | bash -
  - apt-get install -y nodejs

  # --- OpenClaw ---
  - curl -fsSL https://openclaw.ai/install.sh | bash

  # --- Claude Code ---
  - npm install -g @anthropic-ai/claude-code

  # --- Codex CLI ---
  - npm install -g @openai/codex

  # --- API keys (written to .env, NOT in logs/args) ---
  - mkdir -p /root/.openclaw
  - |
    cat > /root/.openclaw/.env <<'ENVEOF'
    ANTHROPIC_API_KEY={anthropic_key}
    OPENAI_API_KEY={openai_key}
    ENVEOF
  - chmod 600 /root/.openclaw/.env

  # --- Systemd unit for OpenClaw gateway ---
  - |
    cat > /etc/systemd/system/openclaw-gateway.service <<'SVCEOF'
    [Unit]
    Description=OpenClaw Gateway
    After=network.target

    [Service]
    Type=simple
    EnvironmentFile=/root/.openclaw/.env
    ExecStart=/usr/local/bin/openclaw gateway start
    Restart=on-failure
    RestartSec=5

    [Install]
    WantedBy=multi-user.target
    SVCEOF
  - systemctl daemon-reload

  # --- Sentinel file: signals completion to the CLI ---
  - touch {sentinel}
"##,
        anthropic_key = anthropic_key,
        openai_key = openai_key,
        sentinel = CLOUD_INIT_SENTINEL,
    )
}
