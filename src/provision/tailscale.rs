use crate::error::AppError;
use crate::provision::commands::ssh_root_async;
use std::path::Path;

pub enum TailscaleProvisionStatus {
    InstalledOnly,
    Connected,
    ConnectFailed(String),
}

/// Optional: Install and configure Tailscale (--tailscale flag).
/// Translated from openclaw-ansible/roles/openclaw/tasks/tailscale-linux.yml.
/// Hardcodes Ubuntu 24.04 (noble) since that's the DO image we use.
/// PProvision.
pub async fn provision(
    ip: &str,
    key: &Path,
    hostname: &str,
    tailscale_auth_key: Option<&str>,
) -> Result<TailscaleProvisionStatus, AppError> {
    // Add Tailscale GPG key + repository
    let add_repo = r#"
curl -fsSL "https://pkgs.tailscale.com/stable/ubuntu/noble.noarmor.gpg" | \
    tee /usr/share/keyrings/tailscale-archive-keyring.gpg > /dev/null && \
curl -fsSL "https://pkgs.tailscale.com/stable/ubuntu/noble.tailscale-keyring.list" | \
    tee /etc/apt/sources.list.d/tailscale.list > /dev/null
"#;
    ssh_root_async(ip, key, add_repo).await.map_err(|e| AppError::Provision {
        phase: "tailscale repo".into(),
        message: e.to_string(),
    })?;

    // Install tailscale
    ssh_root_async(ip, key, "apt-get update && apt-get install -y tailscale").await?;

    // Enable and start tailscaled service
    ssh_root_async(
        ip,
        key,
        "systemctl enable tailscaled && systemctl start tailscaled",
    )
    .await?;

    // Allow Tailscale UDP port through UFW
    ssh_root_async(ip, key, "ufw allow 41641/udp comment 'Tailscale'").await?;

    // Auto-connect if an auth key is provided.
    if let Some(auth_key) = tailscale_auth_key {
        let trimmed = auth_key.trim();
        if !trimmed.is_empty() {
            let up_cmd = format!(
                "tailscale up --auth-key {} --hostname {}",
                shell_quote(trimmed),
                shell_quote(hostname),
            );
            return match ssh_root_async(ip, key, &up_cmd).await {
                Ok(_) => Ok(TailscaleProvisionStatus::Connected),
                Err(e) => Ok(TailscaleProvisionStatus::ConnectFailed(e.to_string())),
            };
        }
    }

    Ok(TailscaleProvisionStatus::InstalledOnly)
}

/// SShell quote.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
