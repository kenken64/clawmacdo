use anyhow::Result;
use clawmacdo_core::config;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_async;
use std::path::Path;

pub struct WhatsAppRepairResult {
    pub supported: bool,
    pub output: String,
}

/// RRepair support.
pub async fn repair_support(ip: &str, key: &Path) -> Result<WhatsAppRepairResult> {
    let cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\" && \
         export HOME=\"{home}\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         echo '[1/6] Updating OpenClaw package...' && \
         (pnpm install -g openclaw@latest 2>&1 || true) && \
         echo '[2/6] Enabling WhatsApp plugin (if available)...' && \
         (openclaw plugins enable whatsapp 2>&1 || true) && \
         if [ -f {home}/.openclaw/openclaw.json ]; then \
           node -e 'const fs=require(\"fs\");const p=process.env.HOME+\"/.openclaw/openclaw.json\";const cfg=JSON.parse(fs.readFileSync(p,\"utf8\"));cfg.channels=cfg.channels||{{}};for(const name of [\"whatsapp\",\"telegram\"]){{if(!cfg.channels[name]) continue;const c=cfg.channels[name];const groupAllow=Array.isArray(c.groupAllowFrom)?c.groupAllowFrom.filter(Boolean):[];const allow=Array.isArray(c.allowFrom)?c.allowFrom.filter(Boolean):[];if(c.groupPolicy===\"allowlist\"&&groupAllow.length===0&&allow.length===0)c.groupPolicy=\"open\";}}fs.writeFileSync(p, JSON.stringify(cfg,null,2)+\"\\n\");'; \
         fi && \
         echo '[3/6] Refreshing bundled extensions...' && \
         OC_EXT=$(find {home}/.local/share/pnpm -path '*/openclaw/extensions' -type d 2>/dev/null | head -1) && \
         if [ -n \"$OC_EXT\" ]; then \
           rm -rf {home}/.openclaw/bundled-extensions && \
           cp -rL \"$OC_EXT\" {home}/.openclaw/bundled-extensions && \
           find {home}/.openclaw/bundled-extensions -type f -links +1 -exec sh -c 'for f do cp -p \"$f\" \"$f.__clawmacdo_tmp\" && mv -f \"$f.__clawmacdo_tmp\" \"$f\"; done' sh {{}} +; \
           echo \"bundled-extensions refreshed from $OC_EXT\"; \
         else \
           echo 'No extensions directory found in PNPM install path.'; \
         fi && \
         echo '[4/6] Reinstalling daemon config...' && \
         (openclaw daemon install --port 18789 --runtime node --force >/dev/null 2>&1 || true) && \
         SVC={home}/.config/systemd/user/openclaw-gateway.service; \
         if [ -f \"$SVC\" ]; then \
           sed -i '/^SupplementaryGroups=/d' \"$SVC\"; \
           sed -i 's/^KillMode=process/KillMode=control-group/' \"$SVC\"; \
           sed -i '/^ExecStart=/{{s|^ExecStart=|ExecStart=/usr/bin/sg docker -c \"|;s|$|\"|;}}' \"$SVC\"; \
         fi && \
         mkdir -p {home}/.config/systemd/user/openclaw-gateway.service.d && \
         printf '[Service]\nEnvironmentFile=-{home}/.openclaw/.env\nEnvironment=OPENCLAW_NO_RESPAWN=1\n' > {home}/.config/systemd/user/openclaw-gateway.service.d/10-env.conf && \
         echo '[5/6] Restarting gateway service...' && \
         (systemctl --user daemon-reload >/dev/null 2>&1 || true) && \
         (systemctl --user restart openclaw-gateway.service >/dev/null 2>&1 || systemctl --user start openclaw-gateway.service >/dev/null 2>&1 || true) && \
         echo -n 'gateway status: ' && (systemctl --user is-active openclaw-gateway.service 2>&1 || true) && \
         echo '[6/6] Probing WhatsApp channel support...' && \
         if command -v timeout >/dev/null 2>&1; then \
           timeout 20s openclaw channels login --channel whatsapp 2>&1 || true; \
         else \
           openclaw channels login --channel whatsapp 2>&1 || true; \
         fi",
        home = config::OPENCLAW_HOME,
    );

    let output = ssh_as_openclaw_async(ip, key, &cmd).await?;
    let lowered = output.to_ascii_lowercase();
    let supported = !lowered.contains("unsupported channel: whatsapp")
        && !lowered.contains("unsupported channel whatsapp");

    Ok(WhatsAppRepairResult { supported, output })
}

#[allow(dead_code)]
pub async fn run(ip: &str, key: &Path) -> Result<()> {
    println!("Repairing WhatsApp support on {ip}...");
    let result = repair_support(ip, key).await?;

    if result.supported {
        println!("WhatsApp channel appears available after repair.\n");
    } else {
        println!(
            "WhatsApp channel is still unsupported after repair. This usually means the installed OpenClaw build does not include WhatsApp.\n"
        );
    }

    println!("{}", result.output.trim());
    Ok(())
}
