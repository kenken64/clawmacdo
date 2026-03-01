use crate::error::AppError;
use crate::provision::commands::ssh_root_async;
use std::path::Path;

/// Step 9: Harden firewall — fail2ban, unattended-upgrades, UFW + DOCKER-USER chain.
/// All packages already installed by cloud-init. This step only writes config files.
/// Translated from openclaw-ansible/roles/openclaw/tasks/firewall-linux.yml.
pub async fn provision(ip: &str, key: &Path, tailscale: bool) -> Result<(), AppError> {
    // --- fail2ban configuration ---
    let fail2ban_cfg = r#"cat > /etc/fail2ban/jail.local << 'F2BEOF'
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
systemctl restart fail2ban && systemctl enable fail2ban"#;
    ssh_root_async(ip, key, fail2ban_cfg)
        .await
        .map_err(|e| AppError::Provision {
            phase: "fail2ban".into(),
            message: e.to_string(),
        })?;

    // --- Unattended-upgrades configuration ---
    let auto_upgrades = r#"cat > /etc/apt/apt.conf.d/20auto-upgrades << 'AUEOF'
APT::Periodic::Update-Package-Lists "1";
APT::Periodic::Unattended-Upgrade "1";
APT::Periodic::AutocleanInterval "7";
AUEOF
"#;
    ssh_root_async(ip, key, auto_upgrades).await?;

    // Use printf to avoid shell interpretation of ${distro_id} etc.
    let unattended = r#"cat > /etc/apt/apt.conf.d/50unattended-upgrades << 'UUEOF'
Unattended-Upgrade::Allowed-Origins {
    "${distro_id}:${distro_codename}-security";
    "${distro_id}ESMApps:${distro_codename}-apps-security";
    "${distro_id}ESM:${distro_codename}-infra-security";
};
Unattended-Upgrade::Package-Blacklist {
};
Unattended-Upgrade::AutoFixInterruptedDpkg "true";
Unattended-Upgrade::MinimalSteps "true";
Unattended-Upgrade::Remove-Unused-Dependencies "true";
Unattended-Upgrade::Automatic-Reboot "false";
UUEOF
"#;
    ssh_root_async(ip, key, unattended).await?;

    // --- UFW: deny routed (blocks Docker from bypassing firewall) ---
    ssh_root_async(ip, key, "ufw default deny routed").await?;

    // --- Tailscale UFW rule (if enabled) ---
    if tailscale {
        ssh_root_async(ip, key, "ufw allow 41641/udp comment 'Tailscale'").await?;
    }

    // --- DOCKER-USER iptables chain in /etc/ufw/after.rules ---
    // Detect default interface dynamically, then insert the DOCKER-USER block before COMMIT.
    let docker_user_rules = r##"
DEFAULT_IF=$(ip route | grep default | awk '{print $5}' | head -n1)
if [ -z "$DEFAULT_IF" ]; then
    echo "ERROR: Could not detect default network interface" >&2
    exit 1
fi

# Check if DOCKER-USER block already exists
if grep -q 'DOCKER-USER' /etc/ufw/after.rules; then
    echo "DOCKER-USER rules already present, skipping"
else
    awk -v default_if="$DEFAULT_IF" '
    /^COMMIT$/ && !inserted {
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
    }
    { print }
    END {
        if (!inserted) {
            print "ERROR: Could not find COMMIT in /etc/ufw/after.rules" > "/dev/stderr"
            exit 1
        }
    }' /etc/ufw/after.rules > /etc/ufw/after.rules.tmp && mv /etc/ufw/after.rules.tmp /etc/ufw/after.rules
fi
"##;
    ssh_root_async(ip, key, docker_user_rules)
        .await
        .map_err(|e| AppError::Provision {
            phase: "DOCKER-USER rules".into(),
            message: e.to_string(),
        })?;

    // --- Reload UFW ---
    ssh_root_async(ip, key, "ufw reload").await?;

    Ok(())
}
