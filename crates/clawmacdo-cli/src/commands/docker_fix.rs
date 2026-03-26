use anyhow::Result;
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::{
    ssh_as_openclaw_async, ssh_as_openclaw_with_user_async, ssh_root_async,
};
use std::path::Path;

pub struct DockerFixResult {
    pub ok: bool,
    pub output: String,
}

/// Install Docker via the official convenience script if it is not present.
async fn ensure_docker_installed(ip: &str, key: &Path) -> Result<String> {
    let check = ssh_root_async(
        ip,
        key,
        "command -v docker >/dev/null 2>&1 && echo installed || echo missing",
    )
    .await?;
    if check.trim() == "missing" {
        let out = ssh_root_async(
            ip,
            key,
            "echo '[0/6] Docker not found — installing...' && curl -fsSL https://get.docker.com | sh && systemctl enable --now docker && echo docker_install_ok",
        )
        .await?;
        Ok(out)
    } else {
        Ok(String::new())
    }
}

pub async fn repair_access(ip: &str, key: &Path, ssh_user: &str) -> Result<DockerFixResult> {
    // Ensure Docker is installed before attempting repair
    let install_output = ensure_docker_installed(ip, key).await?;

    // Restart the systemd user service manager so the openclaw process picks
    // up the docker group added after the service was originally started.
    let uid_cmd = "id -u openclaw";
    let uid_out = ssh_root_async(ip, key, uid_cmd).await.unwrap_or_default();
    let uid = uid_out.trim();
    if !uid.is_empty() {
        let restart_user_mgr = format!(
            "systemctl stop user@{uid}.service 2>/dev/null || true; \
             sleep 1; \
             systemctl start user@{uid}.service 2>/dev/null || true",
        );
        let _ = ssh_root_async(ip, key, &restart_user_mgr).await;
    }

    let cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         echo '[1/6] Verifying docker group membership...' && \
         if id -nG | tr ' ' '\\n' | grep -qx docker; then echo 'docker_group_member=yes'; else echo 'docker_group_member=no'; fi && \
         echo '[2/6] Reinstalling gateway service + docker-group wrapper...' && \
         (openclaw daemon install --port 18789 --runtime node --force >/dev/null 2>&1 || true) && \
         SVC={home}/.config/systemd/user/openclaw-gateway.service; \
         if [ -f \"$SVC\" ]; then \
           sed -i '/^SupplementaryGroups=/d' \"$SVC\"; \
           sed -i 's/^KillMode=process/KillMode=control-group/' \"$SVC\"; \
           sed -i '/^ExecStart=/{{s|^ExecStart=|ExecStart=/usr/bin/sg docker -c \"|;s|$|\"|;}}' \"$SVC\"; \
         fi && \
         mkdir -p {home}/.config/systemd/user/openclaw-gateway.service.d && \
         printf '[Service]\nEnvironmentFile=-{home}/.openclaw/.env\nEnvironment=OPENCLAW_NO_RESPAWN=1\n' > {home}/.config/systemd/user/openclaw-gateway.service.d/10-env.conf && \
         echo '[3/6] Reloading and restarting gateway...' && \
         (systemctl --user daemon-reload >/dev/null 2>&1 || true) && \
         (systemctl --user restart openclaw-gateway.service >/dev/null 2>&1 || systemctl --user start openclaw-gateway.service >/dev/null 2>&1 || true) && \
         echo -n 'gateway_status=' && (systemctl --user is-active openclaw-gateway.service 2>/dev/null || true) && \
         echo '[4/6] Probing docker access as openclaw...' && \
         (docker info >/dev/null 2>&1 && echo docker_direct_ok || echo docker_direct_fail) && \
         (/usr/bin/sg docker -c 'docker info >/dev/null 2>&1 && echo docker_sg_ok || echo docker_sg_fail') && \
         echo '[5/6] Probing sandbox image access...' && \
         (/usr/bin/sg docker -c 'docker image inspect openclaw-sandbox:bookworm-slim >/dev/null 2>&1 && echo sandbox_image_ok || echo sandbox_image_missing') && \
         echo '[6/6] Probing gateway health (waiting for startup)...' && \
         (for i in 1 2 3 4 5; do curl -fsS --max-time 3 http://127.0.0.1:18789/health >/dev/null 2>&1 && echo health_ok && break || sleep 3; done; curl -fsS --max-time 3 http://127.0.0.1:18789/health >/dev/null 2>&1 || echo health_fail)",
        home = config::OPENCLAW_HOME,
    );

    let output = if ssh_user == "root" {
        ssh_as_openclaw_async(ip, key, &cmd).await?
    } else {
        ssh_as_openclaw_with_user_async(ip, key, &cmd, ssh_user).await?
    };

    let combined = if install_output.is_empty() {
        output
    } else {
        format!("{install_output}\n{output}")
    };
    let lowered = combined.to_ascii_lowercase();
    let ok = lowered.contains("docker_sg_ok")
        && (lowered.contains("health_ok")
            || lowered.contains("gateway_status=active")
            || lowered.contains("gateway_status=activating"));
    Ok(DockerFixResult {
        ok,
        output: combined,
    })
}

#[allow(dead_code)]
pub async fn run(ip: &str, key: &Path, ssh_user: &str) -> Result<()> {
    println!("Repairing agent Docker access on {ip}...");
    let result = repair_access(ip, key, ssh_user).await?;

    if result.ok {
        println!("Docker access repair completed.\n");
    } else {
        println!("Repair finished, but checks still indicate an issue.\n");
    }

    println!("{}", result.output.trim());
    Ok(())
}
