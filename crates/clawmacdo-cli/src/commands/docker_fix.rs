use anyhow::Result;
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_async;
use std::path::Path;

pub struct DockerFixResult {
    pub ok: bool,
    pub output: String,
}

pub async fn repair_access(ip: &str, key: &Path) -> Result<DockerFixResult> {
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
           sed -i '/^ExecStart=/{{s|^ExecStart=|ExecStart=/usr/bin/sg docker -c \"|;s|$|\"|;}}' \"$SVC\"; \
         fi && \
         mkdir -p {home}/.config/systemd/user/openclaw-gateway.service.d && \
         printf '[Service]\nEnvironmentFile=-{home}/.openclaw/.env\nEnvironment=OPENCLAW_BUNDLED_PLUGINS_DIR={home}/.openclaw/bundled-extensions\nEnvironment=OPENCLAW_NO_RESPAWN=1\n' > {home}/.config/systemd/user/openclaw-gateway.service.d/10-env.conf && \
         echo '[3/6] Reloading and restarting gateway...' && \
         (systemctl --user daemon-reload >/dev/null 2>&1 || true) && \
         (systemctl --user restart openclaw-gateway.service >/dev/null 2>&1 || systemctl --user start openclaw-gateway.service >/dev/null 2>&1 || true) && \
         echo -n 'gateway_status=' && (systemctl --user is-active openclaw-gateway.service 2>/dev/null || true) && \
         echo '[4/6] Probing docker access as openclaw...' && \
         (docker info >/dev/null 2>&1 && echo docker_direct_ok || echo docker_direct_fail) && \
         (/usr/bin/sg docker -c 'docker info >/dev/null 2>&1 && echo docker_sg_ok || echo docker_sg_fail') && \
         echo '[5/6] Probing sandbox image access...' && \
         (/usr/bin/sg docker -c 'docker image inspect openclaw-sandbox:bookworm-slim >/dev/null 2>&1 && echo sandbox_image_ok || echo sandbox_image_missing') && \
         echo '[6/6] Probing gateway health...' && \
         (curl -fsS --max-time 3 http://127.0.0.1:18789/health >/dev/null 2>&1 && echo health_ok || echo health_fail)",
        home = config::OPENCLAW_HOME,
    );

    let output = ssh_as_openclaw_async(ip, key, &cmd).await?;
    let lowered = output.to_ascii_lowercase();
    let ok = lowered.contains("docker_sg_ok")
        && (lowered.contains("health_ok")
            || lowered.contains("gateway_status=active")
            || lowered.contains("gateway_status=activating"));

    Ok(DockerFixResult { ok, output })
}

pub async fn run(ip: &str, key: &Path) -> Result<()> {
    println!("Repairing agent Docker access on {ip}...");
    let result = repair_access(ip, key).await?;

    if result.ok {
        println!("Docker access repair completed.\n");
    } else {
        println!("Repair finished, but checks still indicate an issue.\n");
    }

    println!("{}", result.output.trim());
    Ok(())
}
