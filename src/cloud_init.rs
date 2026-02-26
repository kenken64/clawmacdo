use crate::config::CLOUD_INIT_SENTINEL;

/// Generate a cloud-init YAML script for the droplet.
///
/// The script installs system packages, Node 24, OpenClaw, Claude Code, Codex,
/// configures UFW, writes API keys, enables loginctl linger for persistent
/// user services, installs a gateway health-check script + cron, and touches
/// a sentinel file on completion.
///
/// NOTE: We do NOT create a systemd unit here — OpenClaw's own installer
/// creates a user-level service at ~/.config/systemd/user/ which takes
/// precedence. Our job is just to ensure the environment is ready.
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

  # --- Enable loginctl linger for root ---
  # Without this, user-level systemd services (openclaw-gateway) get killed
  # when the last SSH session disconnects. This is critical for unattended
  # operation.
  - loginctl enable-linger root

  # --- Gateway health-check script ---
  # Checks every 5 minutes if the gateway is alive (process + RPC probe).
  # Auto-restarts on failure with a double-check to avoid false positives.
  - mkdir -p /root/.openclaw/workspace
  - |
    cat > /root/.openclaw/workspace/openclaw-healthcheck.sh <<'HCEOF'
    #!/bin/bash
    # OpenClaw Gateway Health Check & Auto-Restart
    LOG_PREFIX="[$(date '+%Y-%m-%d %H:%M:%S')]"

    # Check if gateway process is running
    if ! systemctl --user is-active openclaw-gateway.service >/dev/null 2>&1; then
        echo "$LOG_PREFIX Gateway not running. Starting..."
        systemctl --user start openclaw-gateway.service
        sleep 10
        if systemctl --user is-active openclaw-gateway.service >/dev/null 2>&1; then
            echo "$LOG_PREFIX Gateway started successfully."
        else
            echo "$LOG_PREFIX ERROR: Gateway failed to start!"
        fi
        exit 0
    fi

    # Check RPC probe (the real health check)
    RPC_RESULT=$(timeout 10 openclaw gateway status 2>&1 | grep "RPC probe")
    if echo "$RPC_RESULT" | grep -q "ok"; then
        exit 0
    fi

    echo "$LOG_PREFIX RPC probe failed. Retrying in 15s..."
    sleep 15

    # Double-check before restarting
    RPC_RESULT2=$(timeout 10 openclaw gateway status 2>&1 | grep "RPC probe")
    if echo "$RPC_RESULT2" | grep -q "ok"; then
        echo "$LOG_PREFIX RPC probe recovered on retry."
        exit 0
    fi

    echo "$LOG_PREFIX RPC probe still failing. Restarting gateway..."
    systemctl --user restart openclaw-gateway.service
    sleep 15

    if systemctl --user is-active openclaw-gateway.service >/dev/null 2>&1; then
        echo "$LOG_PREFIX Gateway restarted successfully."
    else
        echo "$LOG_PREFIX ERROR: Gateway failed to restart!"
    fi
    HCEOF
  - chmod +x /root/.openclaw/workspace/openclaw-healthcheck.sh

  # --- Cron: health-check every 5 minutes ---
  - |
    (crontab -l 2>/dev/null | grep -v openclaw-healthcheck; echo "*/5 * * * * /root/.openclaw/workspace/openclaw-healthcheck.sh >> /tmp/openclaw-healthcheck.log 2>&1") | crontab -

  # --- Cron: log rotation (keep healthcheck log under 10MB) ---
  - |
    (crontab -l 2>/dev/null | grep -v openclaw-logrotate; echo "0 0 * * * /usr/bin/truncate -s 0 /tmp/openclaw-healthcheck.log") | crontab -

  # --- Sentinel file: signals completion to the CLI ---
  - touch {sentinel}
"##,
        anthropic_key = anthropic_key,
        openai_key = openai_key,
        sentinel = CLOUD_INIT_SENTINEL,
    )
}
