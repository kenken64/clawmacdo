use crate::config::{self, DeployRecord};
use crate::digitalocean::DoClient;
use crate::progress;
use crate::provision::{self, ProvisionOpts};
use crate::{cloud_init, ssh, ui};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::path::PathBuf;
use tokio::sync::mpsc;

/// Parameters for a deploy operation.
pub struct DeployParams {
    pub do_token: String,
    pub anthropic_key: String,
    pub openai_key: String,
    pub gemini_key: String,
    pub whatsapp_phone_number: String,
    pub telegram_bot_token: String,
    pub region: Option<String>,
    pub size: Option<String>,
    pub hostname: Option<String>,
    pub backup: Option<PathBuf>,
    pub enable_backups: bool,
    /// Enable Tailscale VPN on the droplet.
    pub tailscale: bool,
    /// If true, skip interactive prompts and use the pre-set backup path.
    pub non_interactive: bool,
    /// Optional channel for streaming progress to the web UI (SSE).
    /// `None` in CLI mode; `Some(tx)` in serve mode.
    pub progress_tx: Option<mpsc::UnboundedSender<String>>,
}

fn has_value(s: &str) -> bool {
    !s.trim().is_empty()
}

fn build_failover_setup_cmd(openai_enabled: bool, gemini_enabled: bool) -> Option<String> {
    if !openai_enabled && !gemini_enabled {
        return None;
    }

    let home = config::OPENCLAW_HOME;
    let uid = "$(id -u)";
    let mut cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:$PATH\" \
         XDG_RUNTIME_DIR=/run/user/{uid} \
         DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{uid}/bus; ",
    );
    cmd.push_str("openclaw models set anthropic/claude-opus-4-6 >/dev/null 2>&1 || true;");

    if openai_enabled {
        cmd.push_str(" openclaw models fallbacks add openai/gpt-5-mini >/dev/null 2>&1 || true;");
    }
    if gemini_enabled {
        cmd.push_str(" openclaw models fallbacks add google/gemini-2.5-flash >/dev/null 2>&1 || true;");
    }

    cmd.push_str(" echo ok");
    Some(cmd)
}

/// Run the full 16-step deploy flow. Returns the DeployRecord on success.
pub async fn run(params: DeployParams) -> Result<DeployRecord> {
    config::ensure_dirs()?;
    let deploy_id = uuid::Uuid::new_v4().to_string();

    let tx = &params.progress_tx;

    // ── Step 1: Resolve parameters ──────────────────────────────────────
    progress::emit(tx, "\n[Step 1/16] Resolving parameters...");

    let region = match params.region {
        Some(r) => r,
        None if params.non_interactive => config::DEFAULT_REGION.to_string(),
        None => ui::prompt_region()?,
    };
    let size = match params.size {
        Some(s) => s,
        None if params.non_interactive => config::DEFAULT_SIZE.to_string(),
        None => ui::prompt_size()?,
    };
    let hostname = match params.hostname {
        Some(h) => h,
        None if params.non_interactive => format!("openclaw-{}", &deploy_id[..8]),
        None => ui::prompt_hostname(&deploy_id)?,
    };
    let backup_path = if params.non_interactive {
        params.backup
    } else {
        match params.backup {
            Some(p) => Some(p),
            None => ui::prompt_backup()?,
        }
    };

    progress::emit(tx, &format!("  Region:   {region}"));
    progress::emit(tx, &format!("  Size:     {size}"));
    progress::emit(tx, &format!("  Hostname: {hostname}"));
    progress::emit(
        tx,
        &format!(
            "  Backup:   {}",
            backup_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "None".into())
        ),
    );

    // ── Step 2: Generate SSH key pair ───────────────────────────────────
    progress::emit(tx, "\n[Step 2/16] Generating SSH key pair...");

    let keypair = ssh::generate_keypair(&deploy_id)?;
    progress::emit(tx, &format!("  Key saved: {}", keypair.private_key_path.display()));

    // ── Step 3: Upload public key to DO ─────────────────────────────────
    progress::emit(tx, "\n[Step 3/16] Uploading SSH public key to DigitalOcean...");

    let do_client = DoClient::new(&params.do_token)?;
    let key_name = format!("clawmacdo-{}", &deploy_id[..8]);
    let key_info = do_client
        .upload_ssh_key(&key_name, &keypair.public_key_openssh)
        .await
        .context("Failed to upload SSH key to DigitalOcean")?;
    progress::emit(
        tx,
        &format!("  Key ID: {}, Fingerprint: {}", key_info.id, key_info.fingerprint),
    );

    // ── Step 4: Create droplet ──────────────────────────────────────────
    progress::emit(tx, "\n[Step 4/16] Creating droplet with cloud-init...");

    if params.anthropic_key.starts_with("sk-ant-oat") {
        progress::emit(tx, "  Warning: Anthropic key looks like an OAuth token (sk-ant-oat...).");
        progress::emit(tx, "     OAuth tokens are short-lived and break Claude Code.");
        progress::emit(
            tx,
            "     It will NOT be written to .env. Use a real API key (sk-ant-api...) instead.",
        );
        progress::emit(tx, "     OpenClaw gateway auth will still work via openclaw.json profiles.");
    }
    let user_data = cloud_init::generate();
    let droplet = do_client
        .create_droplet(
            &hostname,
            &region,
            &size,
            key_info.id,
            &user_data,
            params.enable_backups,
        )
        .await
        .context("Failed to create droplet")?;

    let droplet_id = droplet.id;
    progress::emit(tx, &format!("  Droplet created: ID {droplet_id}"));

    // From here on, if we fail we print debug info instead of auto-destroying.
    let result = deploy_steps_5_through_16(
        &do_client,
        droplet_id,
        &keypair.private_key_path,
        &keypair.public_key_openssh,
        &key_info.fingerprint,
        backup_path.as_deref(),
        &deploy_id,
        &hostname,
        &region,
        &size,
        &params.anthropic_key,
        &params.openai_key,
        &params.gemini_key,
        &params.whatsapp_phone_number,
        &params.telegram_bot_token,
        params.tailscale,
        &params.progress_tx,
    )
    .await;

    match result {
        Ok(record) => Ok(record),
        Err(e) => {
            // Fetch IP if possible for debug info
            let ip = do_client
                .get_droplet(droplet_id)
                .await
                .ok()
                .and_then(|d| d.public_ip())
                .unwrap_or_else(|| "unknown".into());
            eprintln!("\nDeploy failed: {e:#}");
            eprintln!("\nDroplet was NOT destroyed. Debug info:");
            eprintln!("  Droplet ID: {droplet_id}");
            eprintln!("  IP Address: {ip}");
            eprintln!(
                "  SSH:        ssh -i {} root@{ip}",
                keypair.private_key_path.display()
            );
            bail!("Deploy failed at a post-creation step: {e:#}");
        }
    }
}

async fn deploy_steps_5_through_16(
    do_client: &DoClient,
    droplet_id: u64,
    private_key_path: &std::path::Path,
    public_key_openssh: &str,
    ssh_fingerprint: &str,
    backup_path: Option<&std::path::Path>,
    deploy_id: &str,
    hostname: &str,
    region: &str,
    size: &str,
    anthropic_key: &str,
    openai_key: &str,
    gemini_key: &str,
    whatsapp_phone_number: &str,
    telegram_bot_token: &str,
    tailscale: bool,
    progress_tx: &Option<mpsc::UnboundedSender<String>>,
) -> Result<DeployRecord> {
    let tx = progress_tx;
    // ── Step 5: Poll until droplet is active ────────────────────────────
    progress::emit(tx, "\n[Step 5/16] Waiting for droplet to become active...");
    let sp = ui::spinner("[Step 5/16] Waiting for droplet to become active...");
    let droplet = do_client
        .wait_for_active(droplet_id, std::time::Duration::from_secs(300))
        .await
        .context("Droplet did not become active within 5 minutes")?;
    let ip = droplet.public_ip().unwrap();
    let msg = format!("[Step 5/16] Droplet active at {ip}");
    sp.finish_with_message(msg.clone());
    progress::emit(tx, &msg);

    // ── Step 6: Wait for SSH ────────────────────────────────────────────
    progress::emit(tx, "\n[Step 6/16] Waiting for SSH...");
    let sp = ui::spinner("[Step 6/16] Waiting for SSH...");
    ssh::wait_for_ssh(&ip, private_key_path, std::time::Duration::from_secs(300))
        .await
        .context("SSH did not become available within 5 minutes")?;
    sp.finish_with_message("[Step 6/16] SSH ready");
    progress::emit(tx, "[Step 6/16] SSH ready");

    // ── Step 7: Wait for cloud-init ─────────────────────────────────────
    progress::emit(tx, "\n[Step 7/16] Waiting for cloud-init to finish (this may take a few minutes)...");
    let sp = ui::spinner(
        "[Step 7/16] Waiting for cloud-init to finish (this may take a few minutes)...",
    );
    ssh::wait_for_cloud_init(&ip, private_key_path, std::time::Duration::from_secs(1800))
        .await
        .context("Cloud-init did not complete within 30 minutes")?;
    sp.finish_with_message("[Step 7/16] Cloud-init complete");
    progress::emit(tx, "[Step 7/16] Cloud-init complete");

    // ── Step 8: Upload & restore backup ─────────────────────────────────
    let backup_restored: Option<String>;
    if let Some(bp) = backup_path {
        progress::emit(tx, "\n[Step 8/16] Uploading and restoring backup...");
        let sp = ui::spinner("[Step 8/16] Uploading and restoring backup...");
        let remote_archive = "/tmp/openclaw_backup.tar.gz";
        let ip_clone = ip.clone();
        let key_clone = private_key_path.to_path_buf();
        let bp_clone = bp.to_path_buf();
        tokio::task::spawn_blocking(move || {
            ssh::scp_upload(&ip_clone, &key_clone, &bp_clone, remote_archive)
        })
        .await??;

        // Extract backup — restore to /root/.openclaw temporarily;
        // provision::user will move configs to /home/openclaw/.openclaw later
        let extract_cmd = concat!(
            "mkdir -p /root/.openclaw && ",
            "cd /tmp && tar xzf openclaw_backup.tar.gz && ",
            "cp -a /tmp/openclaw/* /root/.openclaw/ 2>/dev/null; ",
            "rm -rf /tmp/openclaw /tmp/openclaw_backup.tar.gz && ",
            "echo ok"
        );
        let ip_clone = ip.clone();
        let key_clone = private_key_path.to_path_buf();
        tokio::task::spawn_blocking(move || ssh::exec(&ip_clone, &key_clone, extract_cmd))
            .await??;
        sp.finish_with_message("[Step 8/16] Backup uploaded and restored");
        progress::emit(tx, "[Step 8/16] Backup uploaded and restored");
        backup_restored = Some(bp.display().to_string());
    } else {
        progress::emit(tx, "\n[Step 8/16] No backup to restore, skipping.");
        backup_restored = None;
    }

    // ── Steps 9–14: Provision (user, firewall, Docker, Node.js, OpenClaw, Tailscale) ──
    let provision_opts = ProvisionOpts {
        anthropic_key,
        openai_key,
        gemini_key,
        whatsapp_phone_number,
        telegram_bot_token,
        public_key_openssh,
        tailscale,
        progress_tx: tx.clone(),
    };
    provision::run(&ip, private_key_path, &provision_opts)
        .await
        .context("Provision failed")?;

    // ── Step 15: Start gateway as openclaw user ─────────────────────────
    progress::emit(tx, "\n[Step 15/16] Starting OpenClaw gateway (user service)...");
    let sp = ui::spinner("[Step 15/16] Starting OpenClaw gateway (user service)...");
    let openai_onboard_arg = if has_value(openai_key) {
        " --openai-api-key \"$OPENAI_API_KEY\""
    } else {
        ""
    };
    let gemini_onboard_arg = if has_value(gemini_key) {
        " --gemini-api-key \"$GEMINI_API_KEY\""
    } else {
        ""
    };
    let start_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:$PATH\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         if [ -f {home}/.openclaw/.env ]; then set -a; . {home}/.openclaw/.env; set +a; fi && \
         (openclaw onboard --non-interactive --mode local --auth-choice apiKey --anthropic-api-key \"$ANTHROPIC_API_KEY\"{openai_onboard_arg}{gemini_onboard_arg} --secret-input-mode plaintext --gateway-port 18789 --gateway-bind loopback --install-daemon --daemon-runtime node --skip-skills --accept-risk >/dev/null 2>&1 || \
           openclaw daemon install --port 18789 --runtime node --force >/dev/null 2>&1) && \
         if [ -n \"$TELEGRAM_BOT_TOKEN\" ] && [ -f {home}/.openclaw/openclaw.json ]; then \
           node -e 'const fs=require(\"fs\");const p=process.env.HOME+\"/.openclaw/openclaw.json\";const cfg=JSON.parse(fs.readFileSync(p,\"utf8\"));cfg.channels=cfg.channels||{{}};cfg.channels.telegram=cfg.channels.telegram||{{}};cfg.channels.telegram.botToken=process.env.TELEGRAM_BOT_TOKEN;fs.writeFileSync(p, JSON.stringify(cfg,null,2)+\"\\n\");'; \
         fi && \
         mkdir -p {home}/.config/systemd/user/openclaw-gateway.service.d && \
         printf '[Service]\nEnvironmentFile=-{home}/.openclaw/.env\n' > {home}/.config/systemd/user/openclaw-gateway.service.d/10-env.conf && \
         systemctl --user daemon-reload && \
         systemctl --user enable --now openclaw-gateway.service && \
         systemctl --user is-active openclaw-gateway.service >/dev/null && \
         echo ok",
        home = config::OPENCLAW_HOME,
        openai_onboard_arg = openai_onboard_arg,
        gemini_onboard_arg = gemini_onboard_arg,
    );
    let ip_clone = ip.clone();
    let key_clone = private_key_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        provision::commands::ssh_as_openclaw(&ip_clone, &key_clone, &start_cmd)
    })
    .await??;
    sp.finish_with_message("[Step 15/16] Gateway started (user service)");
    progress::emit(tx, "[Step 15/16] Gateway started (user service)");

    let openai_enabled = has_value(openai_key);
    let gemini_enabled = has_value(gemini_key);

    if let Some(failover_cmd) = build_failover_setup_cmd(openai_enabled, gemini_enabled) {
        progress::emit(tx, "[Step 15/16] Configuring model failover chain...");
        let sp = ui::spinner("[Step 15/16] Configuring model failover chain...");
        let ip_clone = ip.clone();
        let key_clone = private_key_path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            provision::commands::ssh_as_openclaw(&ip_clone, &key_clone, &failover_cmd)
        })
        .await??;
        let mut chain = vec!["Anthropic"];
        if openai_enabled {
            chain.push("OpenAI");
        }
        if gemini_enabled {
            chain.push("Gemini");
        }
        let msg = format!(
            "[Step 15/16] Model failover configured ({})",
            chain.join(" -> ")
        );
        sp.finish_with_message(msg.clone());
        progress::emit(tx, &msg);
    }

    // ── Step 16: Save DeployRecord & summary ────────────────────────────
    progress::emit(tx, "\n[Step 16/16] Saving deploy record...");
    let record = DeployRecord {
        id: deploy_id.to_string(),
        droplet_id,
        hostname: hostname.to_string(),
        ip_address: ip.clone(),
        region: region.to_string(),
        size: size.to_string(),
        ssh_key_path: private_key_path.display().to_string(),
        ssh_key_fingerprint: ssh_fingerprint.to_string(),
        backup_restored,
        created_at: Utc::now(),
    };
    let record_path = record.save()?;
    progress::emit(tx, &format!("  Saved: {}", record_path.display()));

    progress::emit(tx, "\n[Step 16/16] Done!");
    ui::print_summary(&record);

    Ok(record)
}
