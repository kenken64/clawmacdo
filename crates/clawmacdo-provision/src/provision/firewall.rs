use crate::provision::commands::ssh_root_as_async;
use clawmacdo_core::error::AppError;
use std::path::Path;

/// Step 10: Harden firewall — fail2ban, unattended-upgrades, UFW + DOCKER-USER chain.
/// All packages already installed by cloud-init. This step only writes config files.
///
/// All commands are batched into a single SSH exec to avoid connection drops
/// during `ufw reload` which can break subsequent SSH sessions.
pub async fn provision(
    ip: &str,
    key: &Path,
    tailscale: bool,
    ssh_user: &str,
) -> Result<(), AppError> {
    let tailscale_rule = if tailscale {
        "ufw allow 41641/udp comment 'Tailscale'"
    } else {
        "true"
    };

    let firewall_script = format!(
        r##"
set -e

# --- fail2ban configuration ---
if command -v fail2ban-server >/dev/null 2>&1; then
  cat > /etc/fail2ban/jail.local << 'F2BEOF'
# OpenClaw security hardening - SSH protection
[DEFAULT]
bantime = 3600
findtime = 600
maxretry = 5
backend = systemd

[sshd]
enabled = true
port = ssh
filter = sshd
F2BEOF
  systemctl restart fail2ban 2>/dev/null && systemctl enable fail2ban 2>/dev/null || true
else
  echo "fail2ban not installed, skipping"
fi

# --- Unattended-upgrades configuration ---
cat > /etc/apt/apt.conf.d/20auto-upgrades << 'AUEOF'
APT::Periodic::Update-Package-Lists "1";
APT::Periodic::Unattended-Upgrade "1";
APT::Periodic::AutocleanInterval "7";
AUEOF

cat > /etc/apt/apt.conf.d/50unattended-upgrades << 'UUEOF'
Unattended-Upgrade::Allowed-Origins {{
    "${{distro_id}}:${{distro_codename}}-security";
    "${{distro_id}}ESMApps:${{distro_codename}}-apps-security";
    "${{distro_id}}ESM:${{distro_codename}}-infra-security";
}};
Unattended-Upgrade::Package-Blacklist {{
}};
Unattended-Upgrade::AutoFixInterruptedDpkg "true";
Unattended-Upgrade::MinimalSteps "true";
Unattended-Upgrade::Remove-Unused-Dependencies "true";
Unattended-Upgrade::Automatic-Reboot "false";
UUEOF

# --- UFW: deny routed (only when Docker is installed) ---
if command -v docker >/dev/null 2>&1; then
  ufw default deny routed
else
  echo "Docker not installed, skipping deny routed"
fi

# --- Tailscale UFW rule ---
{tailscale_rule}

# --- DOCKER-USER iptables chain (only when Docker is installed) ---
if command -v docker >/dev/null 2>&1; then
  DEFAULT_IF=$(ip route | grep default | awk '{{print $5}}' | head -n1)
  if [ -n "$DEFAULT_IF" ] && ! grep -q 'DOCKER-USER' /etc/ufw/after.rules 2>/dev/null; then
    awk -v default_if="$DEFAULT_IF" '
    /^COMMIT$/ && !inserted {{
        print ""
        print "# Docker port isolation - block all forwarded traffic by default"
        print ":DOCKER-USER - [0:0]"
        print ""
        print "# Allow established connections"
        print "-A DOCKER-USER -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT"
        print ""
        print "# Allow localhost"
        print "-A DOCKER-USER -i lo -j ACCEPT"
        print ""
        print "# Block all other forwarded traffic to Docker containers from external interface"
        print "-A DOCKER-USER -i " default_if " -j DROP"
        inserted=1
    }}
    {{ print }}' /etc/ufw/after.rules > /etc/ufw/after.rules.tmp && mv /etc/ufw/after.rules.tmp /etc/ufw/after.rules
  fi
else
  echo "Docker not installed, skipping DOCKER-USER rules"
fi

# --- Reload UFW only if after.rules was modified (Docker present) ---
# ufw reload restarts iptables and drops the current SSH connection.
# Only needed when DOCKER-USER rules were written to /etc/ufw/after.rules.
# Run via nohup so the SSH channel can close cleanly before the reload.
if command -v docker >/dev/null 2>&1; then
  nohup bash -c 'sleep 1 && ufw reload' >/dev/null 2>&1 &
  sleep 2
fi

echo "Firewall hardening complete"
"##
    );

    ssh_root_as_async(ip, key, &firewall_script, ssh_user)
        .await
        .map_err(|e| AppError::Provision {
            phase: "firewall hardening".into(),
            message: e.to_string(),
        })?;

    Ok(())
}
