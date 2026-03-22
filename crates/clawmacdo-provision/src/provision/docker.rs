use crate::provision::commands::ssh_root_as_async;
use clawmacdo_core::config::OPENCLAW_USER;
use clawmacdo_core::error::AppError;
use std::path::Path;

/// Step 11: Configure Docker daemon without granting openclaw direct Docker access.
/// Docker CE is expected from cloud-init, but on some images (e.g. BytePlus)
/// the `docker.io` package is unavailable. When Docker is missing this step
/// installs it via the official convenience script before configuring it.
pub async fn provision(ip: &str, key: &Path, ssh_user: &str) -> Result<(), AppError> {
    // Check if Docker is installed; if not, install it
    let check = ssh_root_as_async(
        ip,
        key,
        "command -v docker >/dev/null 2>&1 && echo yes || echo no",
        ssh_user,
    )
    .await?;
    if check.trim() == "no" {
        ssh_root_as_async(ip, key, "curl -fsSL https://get.docker.com | sh", ssh_user)
            .await
            .map_err(|e| AppError::Provision {
                phase: "docker install".into(),
                message: e.to_string(),
            })?;
    }

    // Write /etc/docker/daemon.json
    let daemon_json = r#"mkdir -p /etc/docker && cat > /etc/docker/daemon.json << 'DJEOF'
{
  "iptables": true,
  "ip-forward": true,
  "userland-proxy": false,
  "live-restore": true,
  "ip6tables": false,
  "log-driver": "json-file",
  "log-opts": {
    "max-size": "10m",
    "max-file": "3"
  },
  "default-address-pools": [
    {
      "base": "172.17.0.0/12",
      "size": 24
    }
  ]
}
DJEOF
"#;
    ssh_root_as_async(ip, key, daemon_json, ssh_user)
        .await
        .map_err(|e| AppError::Provision {
            phase: "docker daemon.json".into(),
            message: e.to_string(),
        })?;

    // Remove any stale direct Docker access. Docker group membership is effectively
    // root-equivalent on Linux, so the gateway should not inherit it.
    ssh_root_as_async(
        ip,
        key,
        &format!("gpasswd -d {OPENCLAW_USER} docker >/dev/null 2>&1 || true"),
        ssh_user,
    )
    .await?;

    // Restart docker to pick up daemon.json changes
    ssh_root_as_async(ip, key, "systemctl restart docker", ssh_user).await?;

    Ok(())
}
