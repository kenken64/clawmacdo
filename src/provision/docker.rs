use crate::config::OPENCLAW_USER;
use crate::error::AppError;
use crate::provision::commands::ssh_root_async;
use std::path::Path;

/// Step 10: Configure Docker daemon and add openclaw to docker group.
/// Docker CE is already installed and running from cloud-init.
/// This step writes daemon.json for hardening and restarts docker.
/// Translated from openclaw-ansible daemon.json.j2 + docker-linux.yml.
pub async fn provision(ip: &str, key: &Path) -> Result<(), AppError> {
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
    ssh_root_async(ip, key, daemon_json)
        .await
        .map_err(|e| AppError::Provision {
            phase: "docker daemon.json".into(),
            message: e.to_string(),
        })?;

    // Add openclaw user to docker group
    ssh_root_async(
        ip,
        key,
        &format!("usermod -aG docker {}", OPENCLAW_USER),
    )
    .await?;

    // Restart docker to pick up daemon.json changes
    ssh_root_async(ip, key, "systemctl restart docker").await?;

    Ok(())
}
