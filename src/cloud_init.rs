use crate::config::CLOUD_INIT_SENTINEL;

/// Generate a cloud-init YAML script for the droplet.
///
/// Cloud-init handles base system setup only: packages, Docker CE, fail2ban,
/// unattended-upgrades, UFW basics, Node.js 24, and the sentinel file.
///
/// All application-level setup (OpenClaw, users, firewall hardening, Docker
/// daemon config) is handled by the provision modules over SSH after
/// cloud-init completes.
/// GGenerate.
pub fn generate() -> String {
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
  - docker.io
  - fail2ban
  - unattended-upgrades

runcmd:
  # --- Firewall (basic - provision modules add DOCKER-USER rules later) ---
  - ufw default deny incoming
  - ufw default allow outgoing
  - ufw allow 22/tcp
  - ufw allow 18789/tcp
  - ufw --force enable

  # --- Node.js 24 LTS via NodeSource ---
  - curl -fsSL https://deb.nodesource.com/setup_24.x | bash -
  - apt-get install -y nodejs

  # --- Enable corepack (ships with Node) for pnpm ---
  - corepack enable

  # --- Enable Docker ---
  - systemctl enable --now docker

  # --- Enable root SSH login (needed for Tencent Cloud which defaults to ubuntu user) ---
  - sed -i 's/^#\?PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config
  - sed -i 's/^#\?PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config.d/*.conf 2>/dev/null || true
  - mkdir -p /root/.ssh && chmod 700 /root/.ssh
  - cp /home/ubuntu/.ssh/authorized_keys /root/.ssh/authorized_keys 2>/dev/null || true
  - chmod 600 /root/.ssh/authorized_keys 2>/dev/null || true
  - systemctl restart sshd || systemctl restart ssh

  # --- Sentinel file: signals completion to the CLI ---
  - touch {sentinel}
"##,
        sentinel = CLOUD_INIT_SENTINEL,
    )
}
