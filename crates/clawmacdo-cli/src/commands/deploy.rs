use anyhow::{bail, Context, Result};
use chrono::Utc;
use clawmacdo_cloud::cloud_init;
use clawmacdo_cloud::digitalocean::DoClient;
#[cfg(feature = "lightsail")]
use clawmacdo_cloud::lightsail_cli::LightsailCliProvider;
use clawmacdo_cloud::tencent::TencentClient;
use clawmacdo_core::config::{self, CloudProviderType, DeployRecord};
use clawmacdo_provision::{self as provision, ProvisionOpts};
use clawmacdo_ssh as ssh;
use clawmacdo_ui::{progress, ui};
use std::path::PathBuf;
use tokio::sync::mpsc;

/// Parameters for a deploy operation.
pub struct DeployParams {
    #[allow(dead_code)]
    pub customer_name: String,
    pub customer_email: String,
    pub provider: String,
    pub do_token: String,
    pub tencent_secret_id: String,
    pub tencent_secret_key: String,
    pub aws_access_key_id: String,
    pub aws_secret_access_key: String,
    pub aws_region: String,
    pub azure_tenant_id: String,
    pub azure_subscription_id: String,
    pub azure_client_id: String,
    pub azure_client_secret: String,
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
    pub enable_sandbox: bool,
    pub tailscale: bool,
    pub tailscale_auth_key: Option<String>,
    pub primary_model: String,
    pub failover_1: String,
    pub failover_2: String,
    pub profile: String,
    pub non_interactive: bool,
    pub progress_tx: Option<mpsc::UnboundedSender<String>>,
}

fn has_value(s: &str) -> bool {
    !s.trim().is_empty()
}

fn split_anthropic_credential(input: &str) -> (String, String) {
    let value = input.trim().to_string();
    if value.starts_with("sk-ant-oat") {
        (String::new(), value)
    } else {
        (value, String::new())
    }
}

fn model_identifier(model: &str) -> Option<&'static str> {
    match model {
        "anthropic" => Some("anthropic/claude-opus-4-6"),
        "openai" => Some("openai/gpt-5-mini"),
        "gemini" => Some("google/gemini-2.5-flash"),
        _ => None,
    }
}

fn build_model_setup_cmd(primary: &str, failovers: &[&str]) -> String {
    let home = config::OPENCLAW_HOME;
    let uid = "$(id -u)";
    let mut cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:$PATH\" \
         XDG_RUNTIME_DIR=/run/user/{uid} \
         DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{uid}/bus; ",
    );
    let primary_id = model_identifier(primary).unwrap_or("anthropic/claude-opus-4-6");
    cmd.push_str(&format!(
        "openclaw models set {primary_id} >/dev/null 2>&1 || true;"
    ));
    for fo in failovers {
        if let Some(fo_id) = model_identifier(fo) {
            cmd.push_str(&format!(
                " openclaw models fallbacks add {fo_id} >/dev/null 2>&1 || true;"
            ));
        }
    }
    cmd.push_str(" echo ok");
    cmd
}

fn build_profile_setup_cmd(profile: &str) -> String {
    let home = config::OPENCLAW_HOME;
    let uid = "$(id -u)";
    let profile_val = match profile {
        "messaging" | "coding" | "full" => profile,
        _ => "full",
    };
    format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:$PATH\" \
         XDG_RUNTIME_DIR=/run/user/{uid} \
         DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{uid}/bus; \
         CFG={home}/.openclaw/openclaw.json; \
         if [ -f \"$CFG\" ]; then \
           node -e 'const fs=require(\"fs\");const p=process.env.HOME+\"/.openclaw/openclaw.json\";\
const cfg=JSON.parse(fs.readFileSync(p,\"utf8\"));\
cfg.tools=cfg.tools||{{}};\
cfg.tools.profile=\"{profile_val}\";\
fs.writeFileSync(p, JSON.stringify(cfg,null,2)+\"\\n\");' && echo ok; \
         else \
           mkdir -p {home}/.openclaw && \
           node -e 'const fs=require(\"fs\");const p=process.env.HOME+\"/.openclaw/openclaw.json\";\
fs.writeFileSync(p, JSON.stringify({{\"tools\":{{\"profile\":\"{profile_val}\"}}}},null,2)+\"\\n\");' && echo ok; \
         fi"
    )
}

/// Collect failover model slugs that have an API key supplied.
fn collect_failovers<'a>(
    failover_1: &'a str,
    failover_2: &'a str,
    anthropic_key: &str,
    openai_key: &str,
    gemini_key: &str,
) -> Vec<&'a str> {
    let mut out = Vec::new();
    for fo in [failover_1, failover_2] {
        if fo.is_empty() {
            continue;
        }
        let keyed = match fo {
            "anthropic" => has_value(anthropic_key),
            "openai" => has_value(openai_key),
            "gemini" => has_value(gemini_key),
            _ => false,
        };
        if keyed {
            out.push(fo);
        }
    }
    out
}

/// Resolve the cloud provider type from the --provider flag.
fn resolve_provider(provider: &str) -> Result<CloudProviderType> {
    match provider {
        "digitalocean" | "do" => Ok(CloudProviderType::DigitalOcean),
        "lightsail" | "aws" => Ok(CloudProviderType::Lightsail),
        "tencent" | "tc" => Ok(CloudProviderType::Tencent),
        "azure" | "az" => Ok(CloudProviderType::Azure),
        _ => bail!("Unknown provider '{provider}'. Use 'digitalocean', 'lightsail', 'tencent', or 'azure'."),
    }
}

/// Run the full deploy flow. Dispatches to provider-specific function.
pub async fn run(params: DeployParams) -> Result<DeployRecord> {
    let provider = resolve_provider(&params.provider)?;
    match provider {
        CloudProviderType::DigitalOcean => run_do(params).await,
        #[cfg(feature = "lightsail")]
        CloudProviderType::Lightsail => run_lightsail(params).await,
        #[cfg(not(feature = "lightsail"))]
        CloudProviderType::Lightsail => {
            bail!("Lightsail support not compiled in. Build with --features lightsail")
        }
        CloudProviderType::Tencent => run_tencent(params).await,
        #[cfg(feature = "azure")]
        CloudProviderType::Azure => run_azure(params).await,
        #[cfg(not(feature = "azure"))]
        CloudProviderType::Azure => {
            bail!("Azure support not compiled in. Build with --features azure")
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// DigitalOcean deploy (unchanged from original)
// ══════════════════════════════════════════════════════════════════════════

async fn run_do(params: DeployParams) -> Result<DeployRecord> {
    config::ensure_dirs()?;
    let deploy_id = uuid::Uuid::new_v4().to_string();
    let tx = &params.progress_tx;

    // Step 1: Resolve parameters
    progress::emit(tx, "\n[Step 1/16] Resolving parameters...");
    let region = params.region.unwrap_or_else(|| {
        if params.non_interactive {
            config::DEFAULT_REGION.to_string()
        } else {
            ui::prompt_region().unwrap_or_else(|_| config::DEFAULT_REGION.to_string())
        }
    });
    let size = params.size.unwrap_or_else(|| {
        if params.non_interactive {
            config::DEFAULT_SIZE.to_string()
        } else {
            ui::prompt_size().unwrap_or_else(|_| config::DEFAULT_SIZE.to_string())
        }
    });
    let hostname = params.hostname.unwrap_or_else(|| {
        if params.non_interactive {
            format!("openclaw-{}", &deploy_id[..8])
        } else {
            ui::prompt_hostname(&deploy_id)
                .unwrap_or_else(|_| format!("openclaw-{}", &deploy_id[..8]))
        }
    });
    let backup_path = if params.non_interactive {
        params.backup
    } else {
        params.backup.or_else(|| ui::prompt_backup().ok().flatten())
    };

    progress::emit(tx, "  Provider: DigitalOcean");
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

    let (anthropic_api_key, anthropic_setup_token) =
        split_anthropic_credential(&params.anthropic_key);

    // Step 2: Generate SSH key pair
    progress::emit(tx, "\n[Step 2/16] Generating SSH key pair...");
    let keypair = ssh::generate_keypair(&deploy_id)?;
    progress::emit(
        tx,
        &format!("  Key saved: {}", keypair.private_key_path.display()),
    );

    // Step 3: Upload public key to DO
    progress::emit(
        tx,
        "\n[Step 3/16] Uploading SSH public key to DigitalOcean...",
    );
    let do_client = DoClient::new(&params.do_token)?;
    let key_name = format!("clawmacdo-{}", &deploy_id[..8]);
    let key_info = do_client
        .upload_ssh_key(&key_name, &keypair.public_key_openssh)
        .await
        .context("Failed to upload SSH key to DigitalOcean")?;
    progress::emit(
        tx,
        &format!(
            "  Key ID: {}, Fingerprint: {}",
            key_info.id, key_info.fingerprint
        ),
    );

    // Step 4: Create droplet
    progress::emit(tx, "\n[Step 4/16] Creating droplet with cloud-init...");
    if has_value(&anthropic_setup_token) {
        progress::emit(tx, "  Detected Anthropic setup token (sk-ant-oat...).");
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
            &params.customer_email,
        )
        .await
        .context("Failed to create droplet")?;
    let droplet_id = droplet.id;
    progress::emit(tx, &format!("  Droplet created: ID {droplet_id}"));

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
        &anthropic_api_key,
        &anthropic_setup_token,
        &params.openai_key,
        &params.gemini_key,
        &params.whatsapp_phone_number,
        &params.telegram_bot_token,
        params.enable_sandbox,
        params.tailscale,
        params.tailscale_auth_key.as_deref(),
        &params.primary_model,
        &params.failover_1,
        &params.failover_2,
        &params.profile,
        &params.progress_tx,
    )
    .await;

    match result {
        Ok(record) => Ok(record),
        Err(e) => {
            let ip = do_client
                .get_droplet(droplet_id)
                .await
                .ok()
                .and_then(|d| d.public_ip())
                .unwrap_or_else(|| "unknown".into());
            eprintln!("\nDeploy failed: {e:#}");
            eprintln!("  Droplet ID: {droplet_id}");
            eprintln!("  IP Address: {ip}");
            eprintln!(
                "  SSH: ssh -i {} root@{ip}",
                keypair.private_key_path.display()
            );
            bail!("Deploy failed at a post-creation step: {e:#}");
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Tencent Cloud deploy (NEW)
// ══════════════════════════════════════════════════════════════════════════

async fn run_tencent(params: DeployParams) -> Result<DeployRecord> {
    config::ensure_dirs()?;
    let deploy_id = uuid::Uuid::new_v4().to_string();
    let tx = &params.progress_tx;

    // Step 1: Resolve parameters
    progress::emit(tx, "\n[Step 1/16] Resolving parameters...");
    let region = params
        .region
        .unwrap_or_else(|| config::DEFAULT_TENCENT_REGION.to_string());
    let size = params
        .size
        .unwrap_or_else(|| config::DEFAULT_TENCENT_INSTANCE_TYPE.to_string());
    let hostname = params
        .hostname
        .unwrap_or_else(|| format!("openclaw-{}", &deploy_id[..8]));
    let backup_path = if params.non_interactive {
        params.backup
    } else {
        params.backup.or_else(|| ui::prompt_backup().ok().flatten())
    };

    progress::emit(tx, "  Provider: Tencent Cloud");
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

    let (anthropic_api_key, anthropic_setup_token) =
        split_anthropic_credential(&params.anthropic_key);

    // Step 2: Generate SSH key pair
    progress::emit(tx, "\n[Step 2/16] Generating SSH key pair...");
    let keypair = ssh::generate_keypair(&deploy_id)?;
    progress::emit(
        tx,
        &format!("  Key saved: {}", keypair.private_key_path.display()),
    );

    // Step 3: Upload public key to Tencent Cloud
    progress::emit(
        tx,
        "\n[Step 3/16] Uploading SSH public key to Tencent Cloud...",
    );
    let tc_client = TencentClient::new(
        &params.tencent_secret_id,
        &params.tencent_secret_key,
        &region,
    )?;
    let key_name = format!("clawmacdo_{}", &deploy_id[..8]);
    let key_info = tc_client
        .import_key_pair(&key_name, &keypair.public_key_openssh)
        .await
        .context("Failed to upload SSH key to Tencent Cloud")?;
    progress::emit(tx, &format!("  Key ID: {}", key_info.id));

    // Step 4: Create CVM instance
    progress::emit(tx, "\n[Step 4/16] Creating CVM instance with cloud-init...");
    if has_value(&anthropic_setup_token) {
        progress::emit(tx, "  Detected Anthropic setup token (sk-ant-oat...).");
    }
    let user_data = cloud_init::generate();
    let user_data_b64 =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &user_data);
    let instance_id = tc_client
        .create_instance(
            &hostname,
            &size,
            config::DEFAULT_TENCENT_IMAGE_ID,
            &key_info.id,
            &user_data_b64,
            &params.customer_email,
        )
        .await
        .context("Failed to create CVM instance")?;
    progress::emit(tx, &format!("  Instance created: {instance_id}"));

    // Step 5: Wait for instance to be RUNNING
    progress::emit(tx, "\n[Step 5/16] Waiting for instance to become active...");
    let sp = ui::spinner("[Step 5/16] Waiting for instance to become active...");
    let instance = tc_client
        .wait_for_running(&instance_id, std::time::Duration::from_secs(300))
        .await
        .context("Instance did not become RUNNING within 5 minutes")?;
    let ip = instance.public_ip.unwrap();
    let msg = format!("[Step 5/16] Instance active at {ip}");
    sp.finish_with_message(msg.clone());
    progress::emit(tx, &msg);

    // Step 6: Wait for SSH
    progress::emit(tx, "\n[Step 6/16] Waiting for SSH...");
    let sp = ui::spinner("[Step 6/16] Waiting for SSH...");
    ssh::wait_for_ssh(
        &ip,
        &keypair.private_key_path,
        std::time::Duration::from_secs(300),
    )
    .await
    .context("SSH did not become available within 5 minutes")?;
    sp.finish_with_message("[Step 6/16] SSH ready");
    progress::emit(tx, "[Step 6/16] SSH ready");

    // Step 7: Wait for cloud-init
    progress::emit(tx, "\n[Step 7/16] Waiting for cloud-init to finish...");
    let sp = ui::spinner("[Step 7/16] Waiting for cloud-init to finish...");
    ssh::wait_for_cloud_init(
        &ip,
        &keypair.private_key_path,
        std::time::Duration::from_secs(1800),
        None,
    )
    .await
    .context("Cloud-init did not complete within 30 minutes")?;
    sp.finish_with_message("[Step 7/16] Cloud-init complete");
    progress::emit(tx, "[Step 7/16] Cloud-init complete");

    // Step 8: Upload & restore backup
    let backup_restored: Option<String>;
    if let Some(bp) = backup_path.as_deref() {
        progress::emit(tx, "\n[Step 8/16] Uploading and restoring backup...");
        let sp = ui::spinner("[Step 8/16] Uploading and restoring backup...");
        let remote_archive = "/tmp/openclaw_backup.tar.gz";
        let ip_c = ip.clone();
        let key_c = keypair.private_key_path.clone();
        let bp_c = bp.to_path_buf();
        tokio::task::spawn_blocking(move || ssh::scp_upload(&ip_c, &key_c, &bp_c, remote_archive))
            .await??;
        let extract_cmd = "mkdir -p /root/.openclaw && cd /tmp && tar xzf openclaw_backup.tar.gz && cp -a /tmp/openclaw/* /root/.openclaw/ 2>/dev/null; rm -rf /tmp/openclaw /tmp/openclaw_backup.tar.gz && echo ok";
        let ip_c = ip.clone();
        let key_c = keypair.private_key_path.clone();
        tokio::task::spawn_blocking(move || ssh::exec(&ip_c, &key_c, extract_cmd)).await??;
        sp.finish_with_message("[Step 8/16] Backup uploaded and restored");
        progress::emit(tx, "[Step 8/16] Backup uploaded and restored");
        backup_restored = Some(bp.display().to_string());
    } else {
        progress::emit(tx, "\n[Step 8/16] No backup to restore, skipping.");
        backup_restored = None;
    }

    // Steps 9–14: Provision (identical SSH-based provisioning)
    let provision_opts = ProvisionOpts {
        anthropic_api_key: &anthropic_api_key,
        anthropic_setup_token: &anthropic_setup_token,
        openai_key: &params.openai_key,
        gemini_key: &params.gemini_key,
        whatsapp_phone_number: &params.whatsapp_phone_number,
        telegram_bot_token: &params.telegram_bot_token,
        public_key_openssh: &keypair.public_key_openssh,
        tailscale: params.tailscale,
        tailscale_auth_key: params.tailscale_auth_key.as_deref(),
        hostname: &hostname,
        ssh_user: None,
        progress_tx: tx.clone(),
    };
    provision::run(&ip, &keypair.private_key_path, &provision_opts)
        .await
        .context("Provision failed")?;

    // Step 15: Start gateway (Tencent path)
    // Key differences from DO: openclaw may be at /usr/bin/openclaw (npm global) or
    // ~/.local/bin/openclaw (pnpm global). We detect and use whichever exists.
    // We also avoid the `sg docker -c` wrapper in ExecStart which causes exit 127/203
    // on some Ubuntu images where sg is not in the systemd service PATH.
    progress::emit(
        tx,
        "\n[Step 15/16] Starting OpenClaw gateway (user service)...",
    );
    let sp = ui::spinner("[Step 15/16] Starting OpenClaw gateway (user service)...");
    let home = config::OPENCLAW_HOME;
    let anthropic_onboard_arg = if has_value(&anthropic_api_key) {
        " --auth-choice apiKey --anthropic-api-key \"$ANTHROPIC_API_KEY\""
    } else {
        ""
    };
    let openai_onboard_arg = if has_value(&params.openai_key) {
        " --openai-api-key \"$OPENAI_API_KEY\""
    } else {
        ""
    };
    let gemini_onboard_arg = if has_value(&params.gemini_key) {
        " --gemini-api-key \"$GEMINI_API_KEY\""
    } else {
        ""
    };
    let sandbox_setup_cmd = if params.enable_sandbox {
        format!(
            "if [ -f {home}/.openclaw/openclaw.json ]; then \
               node -e 'const fs=require(\"fs\");const p=process.env.HOME+\"/.openclaw/openclaw.json\";const cfg=JSON.parse(fs.readFileSync(p,\"utf8\"));cfg.agents=cfg.agents||{{}};cfg.agents.defaults=cfg.agents.defaults||{{}};cfg.agents.defaults.sandbox=cfg.agents.defaults.sandbox||{{}};cfg.agents.defaults.sandbox.mode=\"non-main\";cfg.agents.defaults.sandbox.scope=cfg.agents.defaults.sandbox.scope||\"session\";cfg.agents.defaults.sandbox.workspaceAccess=cfg.agents.defaults.sandbox.workspaceAccess||\"none\";cfg.agents.defaults.sandbox.docker=cfg.agents.defaults.sandbox.docker||{{}};cfg.agents.defaults.sandbox.docker.image=cfg.agents.defaults.sandbox.docker.image||\"openclaw-sandbox:bookworm-slim\";delete cfg.agents.defaults.sandbox.docker.volumes;fs.writeFileSync(p, JSON.stringify(cfg,null,2)+\"\\n\");'; \
             fi && \
             docker image inspect openclaw-sandbox:bookworm-slim >/dev/null 2>&1 || \
              (docker pull openclaw-sandbox:latest >/dev/null 2>&1 && docker tag openclaw-sandbox:latest openclaw-sandbox:bookworm-slim >/dev/null 2>&1)"
        )
    } else {
        "true".to_string()
    };
    let start_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:$PATH\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         if [ ! -S \"$XDG_RUNTIME_DIR/bus\" ]; then dbus-daemon --session --address=\"$DBUS_SESSION_BUS_ADDRESS\" --fork >/dev/null 2>&1 || true; fi && \
         if [ -f {home}/.openclaw/.env ]; then set -a; . {home}/.openclaw/.env; set +a; fi; \
         (openclaw onboard --non-interactive --mode local{anthropic_onboard_arg}{openai_onboard_arg}{gemini_onboard_arg} --secret-input-mode plaintext --gateway-port 18789 --gateway-bind loopback --install-daemon --daemon-runtime node --skip-skills --accept-risk >/dev/null 2>&1 || true); \
         (openclaw doctor --fix >/dev/null 2>&1 || true); \
         if [ -n \"$ANTHROPIC_SETUP_TOKEN\" ]; then \
           (openclaw models auth setup-token --provider anthropic --token \"$ANTHROPIC_SETUP_TOKEN\" >/dev/null 2>&1 || true); \
         fi; \
         OC_BIN=$(command -v openclaw 2>/dev/null || echo /usr/bin/openclaw); \
         SVC={home}/.config/systemd/user/openclaw-gateway.service; \
         mkdir -p {home}/.config/systemd/user; \
         cat > \"$SVC\" << SVCEOF\n\
[Unit]\n\
Description=OpenClaw Gateway\n\
After=network-online.target\n\
Wants=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
WorkingDirectory={home}/.openclaw\n\
ExecStart=$OC_BIN gateway run\n\
Restart=always\n\
RestartSec=5\n\
Environment=HOME={home}\n\
Environment=PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\n\
Environment=OPENCLAW_NO_RESPAWN=1\n\
Environment=NODE_COMPILE_CACHE=/var/tmp/openclaw-compile-cache\n\
EnvironmentFile=-{home}/.openclaw/.env\n\
\n\
[Install]\n\
WantedBy=default.target\n\
SVCEOF\n\
         mkdir -p /var/tmp/openclaw-compile-cache && \
         OC_EXT=$(find {home}/.local/share/pnpm /usr/lib/node_modules -path '*/openclaw/extensions' -type d 2>/dev/null | head -1); \
         if [ -n \"$OC_EXT\" ]; then rm -rf {home}/.openclaw/bundled-extensions && cp -rL \"$OC_EXT\" {home}/.openclaw/bundled-extensions; fi; \
         ({sandbox_setup_cmd}) && \
         (systemctl --user daemon-reload || true) && \
         (systemctl --user enable openclaw-gateway.service || true) && \
         (systemctl --user restart openclaw-gateway.service >/dev/null 2>&1 || systemctl --user start openclaw-gateway.service >/dev/null 2>&1 || true) && \
         for i in $(seq 1 150); do \
           STATE=$(systemctl --user is-active openclaw-gateway.service 2>/dev/null || true); \
           if [ \"$STATE\" = \"active\" ] || curl -fsS --max-time 2 http://127.0.0.1:18789/health >/dev/null 2>&1; then echo ok; exit 0; fi; \
           sleep 1; \
         done; exit 1"
    );
    let ip_c = ip.clone();
    let key_c = keypair.private_key_path.clone();
    let start_result = tokio::task::spawn_blocking(move || {
        provision::commands::ssh_as_openclaw(&ip_c, &key_c, &start_cmd)
    })
    .await?;
    if let Err(e) = start_result {
        bail!("OpenClaw gateway start failed on Tencent instance: {e}");
    }
    sp.finish_with_message("[Step 15/16] Gateway started (user service)");
    progress::emit(tx, "[Step 15/16] Gateway started (user service)");

    // Model setup (primary + failovers)
    let failovers = collect_failovers(
        &params.failover_1,
        &params.failover_2,
        &params.anthropic_key,
        &params.openai_key,
        &params.gemini_key,
    );
    let model_cmd = build_model_setup_cmd(&params.primary_model, &failovers);
    progress::emit(tx, "[Step 15/16] Configuring model setup...");
    let ip_c = ip.clone();
    let key_c = keypair.private_key_path.clone();
    tokio::task::spawn_blocking(move || {
        provision::commands::ssh_as_openclaw(&ip_c, &key_c, &model_cmd)
    })
    .await??;

    // Profile setup (tools.profile in openclaw.json)
    let profile_cmd = build_profile_setup_cmd(&params.profile);
    progress::emit(tx, &format!("[Step 15/16] Setting tools profile to '{}'...", params.profile));
    let ip_c = ip.clone();
    let key_c = keypair.private_key_path.clone();
    tokio::task::spawn_blocking(move || {
        provision::commands::ssh_as_openclaw(&ip_c, &key_c, &profile_cmd)
    })
    .await??;

    // Step 16: Save DeployRecord
    progress::emit(tx, "\n[Step 16/16] Saving deploy record...");
    let record = DeployRecord {
        id: deploy_id.to_string(),
        provider: Some(CloudProviderType::Tencent),
        droplet_id: 0, // Not applicable for Tencent
        instance_id: Some(instance_id),
        hostname: hostname.to_string(),
        ip_address: ip.clone(),
        region: region.to_string(),
        size: size.to_string(),
        ssh_key_path: keypair.private_key_path.display().to_string(),
        ssh_key_fingerprint: String::new(),
        ssh_key_id: Some(key_info.id),
        resource_group: None,
        backup_restored,
        created_at: Utc::now(),
    };
    let record_path = record.save()?;
    progress::emit(tx, &format!("  Saved: {}", record_path.display()));
    progress::emit(tx, "\n[Step 16/16] Done!");
    ui::print_summary(&record);
    Ok(record)
}

// ══════════════════════════════════════════════════════════════════════════
// DO Steps 5-16 (kept from original for backward compat)
// ══════════════════════════════════════════════════════════════════════════

#[allow(clippy::too_many_arguments)]
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
    anthropic_api_key: &str,
    anthropic_setup_token: &str,
    openai_key: &str,
    gemini_key: &str,
    whatsapp_phone_number: &str,
    telegram_bot_token: &str,
    enable_sandbox: bool,
    tailscale: bool,
    tailscale_auth_key: Option<&str>,
    primary_model: &str,
    failover_1: &str,
    failover_2: &str,
    profile: &str,
    progress_tx: &Option<mpsc::UnboundedSender<String>>,
) -> Result<DeployRecord> {
    let tx = progress_tx;

    // Step 5: Poll until droplet is active
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

    // Step 6: Wait for SSH
    progress::emit(tx, "\n[Step 6/16] Waiting for SSH...");
    let sp = ui::spinner("[Step 6/16] Waiting for SSH...");
    ssh::wait_for_ssh(&ip, private_key_path, std::time::Duration::from_secs(300))
        .await
        .context("SSH did not become available within 5 minutes")?;
    sp.finish_with_message("[Step 6/16] SSH ready");
    progress::emit(tx, "[Step 6/16] SSH ready");

    // Step 7: Wait for cloud-init
    progress::emit(tx, "\n[Step 7/16] Waiting for cloud-init to finish...");
    let sp = ui::spinner("[Step 7/16] Waiting for cloud-init to finish...");
    ssh::wait_for_cloud_init(
        &ip,
        private_key_path,
        std::time::Duration::from_secs(1800),
        None,
    )
    .await
    .context("Cloud-init did not complete within 30 minutes")?;
    sp.finish_with_message("[Step 7/16] Cloud-init complete");
    progress::emit(tx, "[Step 7/16] Cloud-init complete");

    // Step 8: Upload & restore backup
    let backup_restored: Option<String>;
    if let Some(bp) = backup_path {
        progress::emit(tx, "\n[Step 8/16] Uploading and restoring backup...");
        let sp = ui::spinner("[Step 8/16] Uploading and restoring backup...");
        let remote_archive = "/tmp/openclaw_backup.tar.gz";
        let ip_c = ip.clone();
        let key_c = private_key_path.to_path_buf();
        let bp_c = bp.to_path_buf();
        tokio::task::spawn_blocking(move || ssh::scp_upload(&ip_c, &key_c, &bp_c, remote_archive))
            .await??;
        let extract_cmd = "mkdir -p /root/.openclaw && cd /tmp && tar xzf openclaw_backup.tar.gz && cp -a /tmp/openclaw/* /root/.openclaw/ 2>/dev/null; rm -rf /tmp/openclaw /tmp/openclaw_backup.tar.gz && echo ok";
        let ip_c = ip.clone();
        let key_c = private_key_path.to_path_buf();
        tokio::task::spawn_blocking(move || ssh::exec(&ip_c, &key_c, extract_cmd)).await??;
        sp.finish_with_message("[Step 8/16] Backup uploaded and restored");
        progress::emit(tx, "[Step 8/16] Backup uploaded and restored");
        backup_restored = Some(bp.display().to_string());
    } else {
        progress::emit(tx, "\n[Step 8/16] No backup to restore, skipping.");
        backup_restored = None;
    }

    // Steps 9-14: Provision
    let provision_opts = ProvisionOpts {
        anthropic_api_key,
        anthropic_setup_token,
        openai_key,
        gemini_key,
        whatsapp_phone_number,
        telegram_bot_token,
        public_key_openssh,
        tailscale,
        tailscale_auth_key,
        hostname,
        ssh_user: None,
        progress_tx: tx.clone(),
    };
    provision::run(&ip, private_key_path, &provision_opts)
        .await
        .context("Provision failed")?;

    // Step 15: Start gateway
    progress::emit(tx, "\n[Step 15/16] Starting OpenClaw gateway...");
    let sp = ui::spinner("[Step 15/16] Starting OpenClaw gateway...");
    let home = config::OPENCLAW_HOME;
    let anthropic_onboard_arg = if has_value(anthropic_api_key) {
        " --auth-choice apiKey --anthropic-api-key \"$ANTHROPIC_API_KEY\""
    } else {
        ""
    };
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
    let sandbox_setup_cmd = if enable_sandbox {
        format!(
            "if [ -f {home}/.openclaw/openclaw.json ]; then \
               node -e 'const fs=require(\"fs\");const p=process.env.HOME+\"/.openclaw/openclaw.json\";const cfg=JSON.parse(fs.readFileSync(p,\"utf8\"));cfg.agents=cfg.agents||{{}};cfg.agents.defaults=cfg.agents.defaults||{{}};cfg.agents.defaults.sandbox=cfg.agents.defaults.sandbox||{{}};cfg.agents.defaults.sandbox.mode=\"non-main\";cfg.agents.defaults.sandbox.scope=cfg.agents.defaults.sandbox.scope||\"session\";cfg.agents.defaults.sandbox.workspaceAccess=cfg.agents.defaults.sandbox.workspaceAccess||\"none\";cfg.agents.defaults.sandbox.docker=cfg.agents.defaults.sandbox.docker||{{}};cfg.agents.defaults.sandbox.docker.image=cfg.agents.defaults.sandbox.docker.image||\"openclaw-sandbox:bookworm-slim\";delete cfg.agents.defaults.sandbox.docker.volumes;fs.writeFileSync(p, JSON.stringify(cfg,null,2)+\"\\n\");'; \
             fi && \
             /usr/bin/sg docker -c 'docker image inspect openclaw-sandbox:bookworm-slim >/dev/null 2>&1 || \
              (docker pull openclaw-sandbox:latest >/dev/null 2>&1 && docker tag openclaw-sandbox:latest openclaw-sandbox:bookworm-slim >/dev/null 2>&1)'"
        )
    } else {
        "true".to_string()
    };
    let start_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:$PATH\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         if [ ! -S \"$XDG_RUNTIME_DIR/bus\" ]; then dbus-daemon --session --address=\"$DBUS_SESSION_BUS_ADDRESS\" --fork >/dev/null 2>&1 || true; fi && \
         if [ -f {home}/.openclaw/.env ]; then set -a; . {home}/.openclaw/.env; set +a; fi; \
         (openclaw onboard --non-interactive --mode local{anthropic_onboard_arg}{openai_onboard_arg}{gemini_onboard_arg} --secret-input-mode plaintext --gateway-port 18789 --gateway-bind loopback --install-daemon --daemon-runtime node --skip-skills --accept-risk >/dev/null 2>&1 || true); \
         (openclaw doctor --fix >/dev/null 2>&1 || true); \
         if [ -n \"$ANTHROPIC_SETUP_TOKEN\" ]; then \
           (openclaw models auth setup-token --provider anthropic --token \"$ANTHROPIC_SETUP_TOKEN\" >/dev/null 2>&1 || true); \
         fi; \
         (openclaw daemon install --port 18789 --runtime node --force || true); \
         SVC={home}/.config/systemd/user/openclaw-gateway.service; \
         if [ -f \"$SVC\" ]; then \
           OC_EXT=$(find {home}/.local/share/pnpm -path '*/openclaw/extensions' -type d 2>/dev/null | head -1); \
           if [ -n \"$OC_EXT\" ]; then rm -rf {home}/.openclaw/bundled-extensions && cp -rL \"$OC_EXT\" {home}/.openclaw/bundled-extensions; fi; \
           sed -i '/^SupplementaryGroups=/d' \"$SVC\"; \
           sed -i '/^ExecStart=/{{s|^ExecStart=|ExecStart=/usr/bin/sg docker -c \"|;s|$|\"|;}}' \"$SVC\"; \
         fi; \
         ({sandbox_setup_cmd}) && \
         mkdir -p {home}/.config/systemd/user/openclaw-gateway.service.d && \
         printf '[Service]\nEnvironmentFile=-{home}/.openclaw/.env\nEnvironment=OPENCLAW_BUNDLED_PLUGINS_DIR={home}/.openclaw/bundled-extensions\nEnvironment=OPENCLAW_NO_RESPAWN=1\n' > {home}/.config/systemd/user/openclaw-gateway.service.d/10-env.conf && \
         (systemctl --user daemon-reload || true) && \
         (systemctl --user enable openclaw-gateway.service || true) && \
         (systemctl --user restart openclaw-gateway.service >/dev/null 2>&1 || systemctl --user start openclaw-gateway.service >/dev/null 2>&1 || true) && \
         for i in $(seq 1 150); do \
           STATE=$(systemctl --user is-active openclaw-gateway.service 2>/dev/null || true); \
           if [ \"$STATE\" = \"active\" ] || curl -fsS --max-time 2 http://127.0.0.1:18789/health >/dev/null 2>&1; then echo ok; exit 0; fi; \
           sleep 1; \
         done; exit 1"
    );
    let ip_c = ip.clone();
    let key_c = private_key_path.to_path_buf();
    let start_result = tokio::task::spawn_blocking(move || {
        provision::commands::ssh_as_openclaw(&ip_c, &key_c, &start_cmd)
    })
    .await?;
    if let Err(e) = start_result {
        bail!("OpenClaw gateway start failed: {e}");
    }
    sp.finish_with_message("[Step 15/16] Gateway started");
    progress::emit(tx, "[Step 15/16] Gateway started");

    // Model setup (primary + failovers)
    let failovers = collect_failovers(
        failover_1,
        failover_2,
        anthropic_api_key,
        openai_key,
        gemini_key,
    );
    let model_cmd = build_model_setup_cmd(primary_model, &failovers);
    progress::emit(tx, "[Step 15/16] Configuring model setup...");
    let ip_c = ip.clone();
    let key_c = private_key_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        provision::commands::ssh_as_openclaw(&ip_c, &key_c, &model_cmd)
    })
    .await??;

    // Profile setup (tools.profile in openclaw.json)
    let profile_cmd = build_profile_setup_cmd(profile);
    progress::emit(tx, &format!("[Step 15/16] Setting tools profile to '{profile}'..."));
    let ip_c = ip.clone();
    let key_c = private_key_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        provision::commands::ssh_as_openclaw(&ip_c, &key_c, &profile_cmd)
    })
    .await??;

    // Step 16: Save DeployRecord
    progress::emit(tx, "\n[Step 16/16] Saving deploy record...");
    let record = DeployRecord {
        id: deploy_id.to_string(),
        provider: Some(CloudProviderType::DigitalOcean),
        droplet_id,
        instance_id: None,
        hostname: hostname.to_string(),
        ip_address: ip.clone(),
        region: region.to_string(),
        size: size.to_string(),
        ssh_key_path: private_key_path.display().to_string(),
        ssh_key_fingerprint: ssh_fingerprint.to_string(),
        ssh_key_id: None,
        resource_group: None,
        backup_restored,
        created_at: Utc::now(),
    };
    let record_path = record.save()?;
    progress::emit(tx, &format!("  Saved: {}", record_path.display()));
    progress::emit(tx, "\n[Step 16/16] Done!");
    ui::print_summary(&record);
    Ok(record)
}

// ══════════════════════════════════════════════════════════════════════════
// AWS Lightsail deploy (CLI-based)
// ══════════════════════════════════════════════════════════════════════════

#[cfg(feature = "lightsail")]
async fn run_lightsail(params: DeployParams) -> Result<DeployRecord> {
    use clawmacdo_cloud::CloudProvider;
    use std::env;

    config::ensure_dirs()?;
    let deploy_id = uuid::Uuid::new_v4().to_string();
    let tx = &params.progress_tx;

    // Step 1: Validate AWS credentials
    progress::emit(tx, "\n[Step 1/16] Validating AWS credentials and CLI...");
    if params.aws_access_key_id.is_empty() || params.aws_secret_access_key.is_empty() {
        bail!("AWS credentials required. Set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY");
    }

    // Set AWS credentials as environment variables for the CLI
    env::set_var("AWS_ACCESS_KEY_ID", &params.aws_access_key_id);
    env::set_var("AWS_SECRET_ACCESS_KEY", &params.aws_secret_access_key);
    env::set_var("AWS_DEFAULT_REGION", &params.aws_region);

    // Initialize Lightsail provider — ensure AWS CLI is available first
    clawmacdo_cloud::lightsail_cli::ensure_aws_cli()?;
    let lightsail = LightsailCliProvider::new(params.aws_region.clone());

    // Step 2: Generate SSH keypair
    progress::emit(tx, "\n[Step 2/16] Generating SSH keypair...");
    let keypair = clawmacdo_ssh::generate_keypair(&deploy_id)?;

    // Step 3: Upload SSH key to AWS Lightsail
    progress::emit(tx, "\n[Step 3/16] Uploading SSH key to AWS Lightsail...");
    let key_name = format!("clawmacdo-{deploy_id}");
    let key_info = lightsail
        .upload_ssh_key(&key_name, &keypair.public_key_openssh)
        .await
        .context("Failed to upload SSH key")?;

    progress::emit(tx, &format!("  → SSH Key: {}", key_info.id));

    // Step 4: Resolve parameters
    progress::emit(tx, "\n[Step 4/16] Resolving parameters...");
    let region = &params.aws_region;
    let size = params.size.unwrap_or_else(|| "s-2vcpu-4gb".to_string());
    let hostname = params.hostname.unwrap_or_else(|| {
        let short_id = deploy_id[..8].to_lowercase();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            % 100_000;
        format!("openclaw-{short_id}-{ts}-prod")
    });

    progress::emit(
        tx,
        &format!("  → Region: {region}, Size: {size}, Name: {hostname}"),
    );

    // Step 5: Generate cloud-init user data
    progress::emit(tx, "\n[Step 5/16] Generating cloud-init user data...");
    let user_data = cloud_init::generate_shell();

    // Step 6: Create Lightsail instance
    progress::emit(tx, "\n[Step 6/16] Creating Lightsail instance...");

    let create_params = clawmacdo_cloud::cloud_provider::CreateInstanceParams {
        name: hostname.to_string(),
        region: region.clone(),
        size: size.to_string(),
        image: "ubuntu_24_04".to_string(), // Ubuntu 24.04 LTS
        ssh_key_id: key_name.clone(),
        user_data,
        tags: vec![],
        customer_email: params.customer_email.clone(),
    };

    let instance_info = lightsail.create_instance(create_params).await?;
    progress::emit(tx, &format!("  → Instance ID: {}", instance_info.id));

    // Step 7: Wait for instance to become active
    progress::emit(tx, "\n[Step 7/16] Waiting for instance to become active...");
    let instance_info = lightsail
        .wait_for_active(&instance_info.id, 600) // 10 minute timeout
        .await?;

    let ip = instance_info
        .public_ip
        .as_ref()
        .context("Instance has no public IP")?;
    progress::emit(tx, &format!("  → IP: {ip}"));

    // Step 8: Wait for SSH (with retries)
    progress::emit(tx, "\n[Step 8/16] Waiting for SSH...");
    let mut ssh_ready = false;
    let mut attempt: u32 = 0;
    // Try up to 30 attempts, sleeping 10s between attempts (total ~5 minutes)
    while attempt < 30 {
        attempt += 1;
        progress::emit(tx, &format!("  → SSH check attempt {attempt}/30"));
        match clawmacdo_ssh::wait_for_ssh(
            ip,
            &keypair.private_key_path,
            std::time::Duration::from_secs(10),
        )
        .await
        {
            Ok(_) => {
                ssh_ready = true;
                break;
            }
            Err(e) => {
                progress::emit(tx, &format!("  SSH not ready: {e}"));
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        }
    }
    if !ssh_ready {
        bail!("Timeout waiting for SSH on {ip}");
    }

    // Wait for cloud-init to complete (Lightsail uses ubuntu user, not root)
    progress::emit(tx, "\n[Step 8/16] Waiting for cloud-init to finish...");
    ssh::wait_for_cloud_init(
        ip,
        &keypair.private_key_path,
        std::time::Duration::from_secs(1800),
        Some("ubuntu"),
    )
    .await
    .context("Cloud-init did not complete within 30 minutes")?;
    progress::emit(tx, "[Step 8/16] Cloud-init complete");

    // Step 9: Upload & restore backup
    let backup_restored: Option<String>;
    if let Some(bp) = params.backup.as_deref() {
        progress::emit(tx, "\n[Step 8/16] Uploading and restoring backup...");
        let remote_archive = "/tmp/openclaw_backup.tar.gz";
        let ip_c = ip.clone();
        let key_c = keypair.private_key_path.clone();
        let bp_c = bp.to_path_buf();
        tokio::task::spawn_blocking(move || {
            ssh::scp_upload_as(&ip_c, &key_c, &bp_c, remote_archive, "ubuntu")
        })
        .await??;
        let extract_cmd = "sudo mkdir -p /root/.openclaw && cd /tmp && sudo tar xzf openclaw_backup.tar.gz && sudo cp -a /tmp/openclaw/* /root/.openclaw/ 2>/dev/null; sudo rm -rf /tmp/openclaw /tmp/openclaw_backup.tar.gz && echo ok";
        let ip_c = ip.clone();
        let key_c = keypair.private_key_path.clone();
        tokio::task::spawn_blocking(move || ssh::exec_as(&ip_c, &key_c, extract_cmd, "ubuntu"))
            .await??;
        progress::emit(tx, "[Step 8/16] Backup uploaded and restored");
        backup_restored = Some(bp.display().to_string());
    } else {
        progress::emit(tx, "\n[Step 8/16] No backup to restore, skipping.");
        backup_restored = None;
    }

    let (anthropic_api_key, anthropic_setup_token) =
        split_anthropic_credential(&params.anthropic_key);

    // Steps 9-14: Provision via shared provisioning flow
    let provision_opts = ProvisionOpts {
        anthropic_api_key: &anthropic_api_key,
        anthropic_setup_token: &anthropic_setup_token,
        openai_key: &params.openai_key,
        gemini_key: &params.gemini_key,
        whatsapp_phone_number: &params.whatsapp_phone_number,
        telegram_bot_token: &params.telegram_bot_token,
        public_key_openssh: &keypair.public_key_openssh,
        hostname: &hostname,
        tailscale: params.tailscale,
        tailscale_auth_key: params.tailscale_auth_key.as_deref(),
        ssh_user: Some("ubuntu"),
        progress_tx: tx.clone(),
    };
    provision::run(ip, &keypair.private_key_path, &provision_opts)
        .await
        .context("Provision failed")?;

    // Step 15: Start gateway (Lightsail path — same as Tencent, uses ubuntu SSH user)
    progress::emit(
        tx,
        "\n[Step 15/16] Starting OpenClaw gateway (user service)...",
    );
    let home = config::OPENCLAW_HOME;
    let anthropic_onboard_arg = if has_value(&anthropic_api_key) {
        " --auth-choice apiKey --anthropic-api-key \"$ANTHROPIC_API_KEY\""
    } else {
        ""
    };
    let openai_onboard_arg = if has_value(&params.openai_key) {
        " --openai-api-key \"$OPENAI_API_KEY\""
    } else {
        ""
    };
    let gemini_onboard_arg = if has_value(&params.gemini_key) {
        " --gemini-api-key \"$GEMINI_API_KEY\""
    } else {
        ""
    };
    let sandbox_setup_cmd = if params.enable_sandbox {
        format!(
            "if [ -f {home}/.openclaw/openclaw.json ]; then \
               node -e 'const fs=require(\"fs\");const p=process.env.HOME+\"/.openclaw/openclaw.json\";const cfg=JSON.parse(fs.readFileSync(p,\"utf8\"));cfg.agents=cfg.agents||{{}};cfg.agents.defaults=cfg.agents.defaults||{{}};cfg.agents.defaults.sandbox=cfg.agents.defaults.sandbox||{{}};cfg.agents.defaults.sandbox.mode=\"non-main\";cfg.agents.defaults.sandbox.scope=cfg.agents.defaults.sandbox.scope||\"session\";cfg.agents.defaults.sandbox.workspaceAccess=cfg.agents.defaults.sandbox.workspaceAccess||\"none\";cfg.agents.defaults.sandbox.docker=cfg.agents.defaults.sandbox.docker||{{}};cfg.agents.defaults.sandbox.docker.image=cfg.agents.defaults.sandbox.docker.image||\"openclaw-sandbox:bookworm-slim\";delete cfg.agents.defaults.sandbox.docker.volumes;fs.writeFileSync(p, JSON.stringify(cfg,null,2)+\"\\n\");'; \
             fi && \
             docker image inspect openclaw-sandbox:bookworm-slim >/dev/null 2>&1 || \
              (docker pull openclaw-sandbox:latest >/dev/null 2>&1 && docker tag openclaw-sandbox:latest openclaw-sandbox:bookworm-slim >/dev/null 2>&1)"
        )
    } else {
        "true".to_string()
    };
    let start_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:$PATH\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         if [ ! -S \"$XDG_RUNTIME_DIR/bus\" ]; then dbus-daemon --session --address=\"$DBUS_SESSION_BUS_ADDRESS\" --fork >/dev/null 2>&1 || true; fi && \
         if [ -f {home}/.openclaw/.env ]; then set -a; . {home}/.openclaw/.env; set +a; fi; \
         (openclaw onboard --non-interactive --mode local{anthropic_onboard_arg}{openai_onboard_arg}{gemini_onboard_arg} --secret-input-mode plaintext --gateway-port 18789 --gateway-bind loopback --install-daemon --daemon-runtime node --skip-skills --accept-risk >/dev/null 2>&1 || true); \
         (openclaw doctor --fix >/dev/null 2>&1 || true); \
         if [ -n \"$ANTHROPIC_SETUP_TOKEN\" ]; then \
           (openclaw models auth setup-token --provider anthropic --token \"$ANTHROPIC_SETUP_TOKEN\" >/dev/null 2>&1 || true); \
         fi; \
         OC_BIN=$(command -v openclaw 2>/dev/null || echo /usr/bin/openclaw); \
         SVC={home}/.config/systemd/user/openclaw-gateway.service; \
         mkdir -p {home}/.config/systemd/user; \
         cat > \"$SVC\" << SVCEOF\n\
[Unit]\n\
Description=OpenClaw Gateway\n\
After=network-online.target\n\
Wants=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
WorkingDirectory={home}/.openclaw\n\
ExecStart=$OC_BIN gateway run\n\
Restart=always\n\
RestartSec=5\n\
Environment=HOME={home}\n\
Environment=PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\n\
Environment=OPENCLAW_NO_RESPAWN=1\n\
Environment=NODE_COMPILE_CACHE=/var/tmp/openclaw-compile-cache\n\
EnvironmentFile=-{home}/.openclaw/.env\n\
\n\
[Install]\n\
WantedBy=default.target\n\
SVCEOF\n\
         mkdir -p /var/tmp/openclaw-compile-cache && \
         OC_EXT=$(find {home}/.local/share/pnpm /usr/lib/node_modules -path '*/openclaw/extensions' -type d 2>/dev/null | head -1); \
         if [ -n \"$OC_EXT\" ]; then rm -rf {home}/.openclaw/bundled-extensions && cp -rL \"$OC_EXT\" {home}/.openclaw/bundled-extensions; fi; \
         ({sandbox_setup_cmd}) && \
         (systemctl --user daemon-reload || true) && \
         (systemctl --user enable openclaw-gateway.service || true) && \
         (systemctl --user restart openclaw-gateway.service >/dev/null 2>&1 || systemctl --user start openclaw-gateway.service >/dev/null 2>&1 || true) && \
         for i in $(seq 1 150); do \
           STATE=$(systemctl --user is-active openclaw-gateway.service 2>/dev/null || true); \
           if [ \"$STATE\" = \"active\" ] || curl -fsS --max-time 2 http://127.0.0.1:18789/health >/dev/null 2>&1; then echo ok; exit 0; fi; \
           sleep 1; \
         done; exit 1"
    );
    let ip_c = ip.clone();
    let key_c = keypair.private_key_path.clone();
    let start_result = tokio::task::spawn_blocking(move || {
        provision::commands::ssh_as_openclaw_with_user(&ip_c, &key_c, &start_cmd, "ubuntu")
    })
    .await?;
    if let Err(e) = start_result {
        bail!("OpenClaw gateway start failed on Lightsail instance: {e}");
    }
    progress::emit(tx, "[Step 15/16] Gateway started (user service)");

    // Model setup (primary + failovers)
    let failovers = collect_failovers(
        &params.failover_1,
        &params.failover_2,
        &params.anthropic_key,
        &params.openai_key,
        &params.gemini_key,
    );
    let model_cmd = build_model_setup_cmd(&params.primary_model, &failovers);
    progress::emit(tx, "[Step 15/16] Configuring model setup...");
    let ip_c = ip.clone();
    let key_c = keypair.private_key_path.clone();
    tokio::task::spawn_blocking(move || {
        provision::commands::ssh_as_openclaw_with_user(&ip_c, &key_c, &model_cmd, "ubuntu")
    })
    .await??;

    // Profile setup (tools.profile in openclaw.json)
    let profile_cmd = build_profile_setup_cmd(&params.profile);
    progress::emit(tx, &format!("[Step 15/16] Setting tools profile to '{}'...", params.profile));
    let ip_c = ip.clone();
    let key_c = keypair.private_key_path.clone();
    tokio::task::spawn_blocking(move || {
        provision::commands::ssh_as_openclaw_with_user(&ip_c, &key_c, &profile_cmd, "ubuntu")
    })
    .await??;

    // Step 16: Save DeployRecord
    progress::emit(tx, "\n[Step 16/16] Saving deploy record...");
    let record = DeployRecord {
        id: deploy_id,
        provider: Some(CloudProviderType::Lightsail),
        droplet_id: 0, // Not applicable for Lightsail
        instance_id: Some(instance_info.id.clone()),
        hostname: hostname.to_string(),
        ip_address: ip.clone(),
        region: region.to_string(),
        size: size.to_string(),
        ssh_key_path: keypair.private_key_path.display().to_string(),
        ssh_key_fingerprint: String::new(),
        ssh_key_id: Some(key_name),
        resource_group: None,
        backup_restored,
        created_at: Utc::now(),
    };
    let record_path = record.save()?;
    progress::emit(tx, &format!("  Saved: {}", record_path.display()));
    progress::emit(tx, "\n[Step 16/16] Done!");
    progress::emit(tx, "\n[🚀 Done!] Lightsail deployment complete!");
    ui::print_summary(&record);
    Ok(record)
}

// ══════════════════════════════════════════════════════════════════════════
// Azure Compute VM deploy (NEW)
// ══════════════════════════════════════════════════════════════════════════

#[cfg(feature = "azure")]
async fn run_azure(params: DeployParams) -> Result<DeployRecord> {
    use clawmacdo_cloud::azure_cli::{self, AzureCliProvider};
    use clawmacdo_cloud::CloudProvider;

    config::ensure_dirs()?;
    let deploy_id = uuid::Uuid::new_v4().to_string();
    let tx = &params.progress_tx;

    // Step 1: Validate Azure credentials and CLI
    progress::emit(tx, "\n[Step 1/16] Validating Azure credentials and CLI...");
    if params.azure_tenant_id.is_empty()
        || params.azure_subscription_id.is_empty()
        || params.azure_client_id.is_empty()
        || params.azure_client_secret.is_empty()
    {
        bail!("Azure credentials required: tenant ID, subscription ID, client ID, and client secret");
    }

    azure_cli::ensure_az_cli()?;
    azure_cli::az_login(
        &params.azure_tenant_id,
        &params.azure_client_id,
        &params.azure_client_secret,
    )?;
    azure_cli::az_set_subscription(&params.azure_subscription_id)?;
    progress::emit(tx, "  Azure CLI authenticated.");

    // Step 2: Generate SSH keypair
    progress::emit(tx, "\n[Step 2/16] Generating SSH keypair...");
    let keypair = ssh::generate_keypair(&deploy_id)?;
    progress::emit(
        tx,
        &format!("  Key saved: {}", keypair.private_key_path.display()),
    );

    // Step 3: Upload SSH key (no-op for Azure — passed inline to az vm create)
    progress::emit(tx, "\n[Step 3/16] SSH key will be passed inline to Azure VM...");
    let key_name = format!("clawmacdo-{}", &deploy_id[..8]);
    progress::emit(tx, &format!("  Key name: {key_name}"));

    // Step 4: Resolve parameters and create VM
    let region = params
        .region
        .unwrap_or_else(|| config::DEFAULT_AZURE_REGION.to_string());
    let size = params
        .size
        .unwrap_or_else(|| config::DEFAULT_AZURE_SIZE.to_string());
    let hostname = params
        .hostname
        .unwrap_or_else(|| format!("openclaw-{}", &deploy_id[..8]));
    let backup_path = if params.non_interactive {
        params.backup
    } else {
        params.backup.or_else(|| ui::prompt_backup().ok().flatten())
    };
    let resource_group = format!("clawmacdo-{}", &deploy_id[..8]);

    progress::emit(tx, "\n[Step 4/16] Creating Azure VM with cloud-init...");
    progress::emit(tx, "  Provider: Microsoft Azure");
    progress::emit(tx, &format!("  Region:         {region}"));
    progress::emit(tx, &format!("  Size:           {size}"));
    progress::emit(tx, &format!("  Hostname:       {hostname}"));
    progress::emit(tx, &format!("  Resource Group: {resource_group}"));
    progress::emit(
        tx,
        &format!(
            "  Backup:         {}",
            backup_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "None".into())
        ),
    );

    let (anthropic_api_key, anthropic_setup_token) =
        split_anthropic_credential(&params.anthropic_key);

    if has_value(&anthropic_setup_token) {
        progress::emit(tx, "  Detected Anthropic setup token (sk-ant-oat...).");
    }

    let azure = AzureCliProvider::new(
        region.clone(),
        resource_group.clone(),
        params.azure_subscription_id.clone(),
    );

    // Ensure resource group exists
    azure.ensure_resource_group()?;
    progress::emit(tx, &format!("  Resource group '{resource_group}' ready."));

    // Generate cloud-init for azureuser
    let user_data = cloud_init::generate_for_user("azureuser");

    let create_params = clawmacdo_cloud::cloud_provider::CreateInstanceParams {
        name: hostname.clone(),
        region: region.clone(),
        size: size.clone(),
        image: config::DEFAULT_AZURE_IMAGE.to_string(),
        ssh_key_id: keypair.public_key_openssh.clone(), // Azure uses actual key content
        user_data,
        tags: vec![],
        customer_email: params.customer_email.clone(),
    };

    let instance_info = azure.create_instance(create_params).await?;
    progress::emit(tx, &format!("  VM created: {}", instance_info.name));

    // Step 5: Wait for VM to become active
    progress::emit(tx, "\n[Step 5/16] Waiting for VM to become active...");
    let sp = ui::spinner("[Step 5/16] Waiting for VM to become active...");
    let instance_info = azure
        .wait_for_active(&instance_info.name, 600)
        .await?;
    let ip = instance_info
        .public_ip
        .as_ref()
        .context("VM has no public IP")?
        .clone();
    let msg = format!("[Step 5/16] VM active at {ip}");
    sp.finish_with_message(msg.clone());
    progress::emit(tx, &msg);

    // Step 6: Wait for SSH (with retries — Azure VMs can take a while)
    progress::emit(tx, "\n[Step 6/16] Waiting for SSH...");
    let sp = ui::spinner("[Step 6/16] Waiting for SSH...");
    let mut ssh_ready = false;
    let mut attempt: u32 = 0;
    while attempt < 30 {
        attempt += 1;
        progress::emit(tx, &format!("  SSH check attempt {attempt}/30"));
        match ssh::wait_for_ssh(
            &ip,
            &keypair.private_key_path,
            std::time::Duration::from_secs(10),
        )
        .await
        {
            Ok(_) => {
                ssh_ready = true;
                break;
            }
            Err(e) => {
                progress::emit(tx, &format!("  SSH not ready: {e}"));
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        }
    }
    if !ssh_ready {
        bail!("Timeout waiting for SSH on {ip}");
    }
    sp.finish_with_message("[Step 6/16] SSH ready");
    progress::emit(tx, "[Step 6/16] SSH ready");

    // Step 7: Wait for cloud-init to complete (Azure uses azureuser)
    progress::emit(tx, "\n[Step 7/16] Waiting for cloud-init to finish...");
    let sp = ui::spinner("[Step 7/16] Waiting for cloud-init to finish...");
    ssh::wait_for_cloud_init(
        &ip,
        &keypair.private_key_path,
        std::time::Duration::from_secs(1800),
        Some("azureuser"),
    )
    .await
    .context("Cloud-init did not complete within 30 minutes")?;
    sp.finish_with_message("[Step 7/16] Cloud-init complete");
    progress::emit(tx, "[Step 7/16] Cloud-init complete");

    // Step 8: Upload & restore backup
    let backup_restored: Option<String>;
    if let Some(bp) = backup_path.as_deref() {
        progress::emit(tx, "\n[Step 8/16] Uploading and restoring backup...");
        let remote_archive = "/tmp/openclaw_backup.tar.gz";
        let ip_c = ip.clone();
        let key_c = keypair.private_key_path.clone();
        let bp_c = bp.to_path_buf();
        tokio::task::spawn_blocking(move || {
            ssh::scp_upload_as(&ip_c, &key_c, &bp_c, remote_archive, "azureuser")
        })
        .await??;
        let extract_cmd = "sudo mkdir -p /root/.openclaw && cd /tmp && sudo tar xzf openclaw_backup.tar.gz && sudo cp -a /tmp/openclaw/* /root/.openclaw/ 2>/dev/null; sudo rm -rf /tmp/openclaw /tmp/openclaw_backup.tar.gz && echo ok";
        let ip_c = ip.clone();
        let key_c = keypair.private_key_path.clone();
        tokio::task::spawn_blocking(move || ssh::exec_as(&ip_c, &key_c, extract_cmd, "azureuser"))
            .await??;
        progress::emit(tx, "[Step 8/16] Backup uploaded and restored");
        backup_restored = Some(bp.display().to_string());
    } else {
        progress::emit(tx, "\n[Step 8/16] No backup to restore, skipping.");
        backup_restored = None;
    }

    // Steps 9-14: Provision via shared provisioning flow
    let provision_opts = ProvisionOpts {
        anthropic_api_key: &anthropic_api_key,
        anthropic_setup_token: &anthropic_setup_token,
        openai_key: &params.openai_key,
        gemini_key: &params.gemini_key,
        whatsapp_phone_number: &params.whatsapp_phone_number,
        telegram_bot_token: &params.telegram_bot_token,
        public_key_openssh: &keypair.public_key_openssh,
        hostname: &hostname,
        tailscale: params.tailscale,
        tailscale_auth_key: params.tailscale_auth_key.as_deref(),
        ssh_user: Some("azureuser"),
        progress_tx: tx.clone(),
    };
    provision::run(&ip, &keypair.private_key_path, &provision_opts)
        .await
        .context("Provision failed")?;

    // Step 15: Start gateway (Azure path — same pattern as Lightsail, uses azureuser SSH user)
    progress::emit(
        tx,
        "\n[Step 15/16] Starting OpenClaw gateway (user service)...",
    );
    let sp = ui::spinner("[Step 15/16] Starting OpenClaw gateway (user service)...");
    let home = config::OPENCLAW_HOME;
    let anthropic_onboard_arg = if has_value(&anthropic_api_key) {
        " --auth-choice apiKey --anthropic-api-key \"$ANTHROPIC_API_KEY\""
    } else {
        ""
    };
    let openai_onboard_arg = if has_value(&params.openai_key) {
        " --openai-api-key \"$OPENAI_API_KEY\""
    } else {
        ""
    };
    let gemini_onboard_arg = if has_value(&params.gemini_key) {
        " --gemini-api-key \"$GEMINI_API_KEY\""
    } else {
        ""
    };
    let sandbox_setup_cmd = if params.enable_sandbox {
        format!(
            "if [ -f {home}/.openclaw/openclaw.json ]; then \
               node -e 'const fs=require(\"fs\");const p=process.env.HOME+\"/.openclaw/openclaw.json\";const cfg=JSON.parse(fs.readFileSync(p,\"utf8\"));cfg.agents=cfg.agents||{{}};cfg.agents.defaults=cfg.agents.defaults||{{}};cfg.agents.defaults.sandbox=cfg.agents.defaults.sandbox||{{}};cfg.agents.defaults.sandbox.mode=\"non-main\";cfg.agents.defaults.sandbox.scope=cfg.agents.defaults.sandbox.scope||\"session\";cfg.agents.defaults.sandbox.workspaceAccess=cfg.agents.defaults.sandbox.workspaceAccess||\"none\";cfg.agents.defaults.sandbox.docker=cfg.agents.defaults.sandbox.docker||{{}};cfg.agents.defaults.sandbox.docker.image=cfg.agents.defaults.sandbox.docker.image||\"openclaw-sandbox:bookworm-slim\";delete cfg.agents.defaults.sandbox.docker.volumes;fs.writeFileSync(p, JSON.stringify(cfg,null,2)+\"\\n\");'; \
             fi && \
             docker image inspect openclaw-sandbox:bookworm-slim >/dev/null 2>&1 || \
              (docker pull openclaw-sandbox:latest >/dev/null 2>&1 && docker tag openclaw-sandbox:latest openclaw-sandbox:bookworm-slim >/dev/null 2>&1)"
        )
    } else {
        "true".to_string()
    };
    let start_cmd = format!(
        "export PATH=\"{home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:$PATH\" && \
         export XDG_RUNTIME_DIR=/run/user/$(id -u) DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus && \
         if [ ! -S \"$XDG_RUNTIME_DIR/bus\" ]; then dbus-daemon --session --address=\"$DBUS_SESSION_BUS_ADDRESS\" --fork >/dev/null 2>&1 || true; fi && \
         if [ -f {home}/.openclaw/.env ]; then set -a; . {home}/.openclaw/.env; set +a; fi; \
         (openclaw onboard --non-interactive --mode local{anthropic_onboard_arg}{openai_onboard_arg}{gemini_onboard_arg} --secret-input-mode plaintext --gateway-port 18789 --gateway-bind loopback --install-daemon --daemon-runtime node --skip-skills --accept-risk >/dev/null 2>&1 || true); \
         (openclaw doctor --fix >/dev/null 2>&1 || true); \
         if [ -n \"$ANTHROPIC_SETUP_TOKEN\" ]; then \
           (openclaw models auth setup-token --provider anthropic --token \"$ANTHROPIC_SETUP_TOKEN\" >/dev/null 2>&1 || true); \
         fi; \
         OC_BIN=$(command -v openclaw 2>/dev/null || echo /usr/bin/openclaw); \
         SVC={home}/.config/systemd/user/openclaw-gateway.service; \
         mkdir -p {home}/.config/systemd/user; \
         cat > \"$SVC\" << SVCEOF\n\
[Unit]\n\
Description=OpenClaw Gateway\n\
After=network-online.target\n\
Wants=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
WorkingDirectory={home}/.openclaw\n\
ExecStart=$OC_BIN gateway run\n\
Restart=always\n\
RestartSec=5\n\
Environment=HOME={home}\n\
Environment=PATH={home}/.local/bin:{home}/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin\n\
Environment=OPENCLAW_NO_RESPAWN=1\n\
Environment=NODE_COMPILE_CACHE=/var/tmp/openclaw-compile-cache\n\
EnvironmentFile=-{home}/.openclaw/.env\n\
\n\
[Install]\n\
WantedBy=default.target\n\
SVCEOF\n\
         mkdir -p /var/tmp/openclaw-compile-cache && \
         OC_EXT=$(find {home}/.local/share/pnpm /usr/lib/node_modules -path '*/openclaw/extensions' -type d 2>/dev/null | head -1); \
         if [ -n \"$OC_EXT\" ]; then rm -rf {home}/.openclaw/bundled-extensions && cp -rL \"$OC_EXT\" {home}/.openclaw/bundled-extensions; fi; \
         ({sandbox_setup_cmd}) && \
         (systemctl --user daemon-reload || true) && \
         (systemctl --user enable openclaw-gateway.service || true) && \
         (systemctl --user restart openclaw-gateway.service >/dev/null 2>&1 || systemctl --user start openclaw-gateway.service >/dev/null 2>&1 || true) && \
         for i in $(seq 1 150); do \
           STATE=$(systemctl --user is-active openclaw-gateway.service 2>/dev/null || true); \
           if [ \"$STATE\" = \"active\" ] || curl -fsS --max-time 2 http://127.0.0.1:18789/health >/dev/null 2>&1; then echo ok; exit 0; fi; \
           sleep 1; \
         done; exit 1"
    );
    let ip_c = ip.clone();
    let key_c = keypair.private_key_path.clone();
    let start_result = tokio::task::spawn_blocking(move || {
        provision::commands::ssh_as_openclaw_with_user(&ip_c, &key_c, &start_cmd, "azureuser")
    })
    .await?;
    if let Err(e) = start_result {
        bail!("OpenClaw gateway start failed on Azure VM: {e}");
    }
    sp.finish_with_message("[Step 15/16] Gateway started (user service)");
    progress::emit(tx, "[Step 15/16] Gateway started (user service)");

    // Model setup (primary + failovers)
    let failovers = collect_failovers(
        &params.failover_1,
        &params.failover_2,
        &params.anthropic_key,
        &params.openai_key,
        &params.gemini_key,
    );
    let model_cmd = build_model_setup_cmd(&params.primary_model, &failovers);
    progress::emit(tx, "[Step 15/16] Configuring model setup...");
    let ip_c = ip.clone();
    let key_c = keypair.private_key_path.clone();
    tokio::task::spawn_blocking(move || {
        provision::commands::ssh_as_openclaw_with_user(&ip_c, &key_c, &model_cmd, "azureuser")
    })
    .await??;

    // Profile setup (tools.profile in openclaw.json)
    let profile_cmd = build_profile_setup_cmd(&params.profile);
    progress::emit(tx, &format!("[Step 15/16] Setting tools profile to '{}'...", params.profile));
    let ip_c = ip.clone();
    let key_c = keypair.private_key_path.clone();
    tokio::task::spawn_blocking(move || {
        provision::commands::ssh_as_openclaw_with_user(&ip_c, &key_c, &profile_cmd, "azureuser")
    })
    .await??;

    // Step 16: Save DeployRecord
    progress::emit(tx, "\n[Step 16/16] Saving deploy record...");
    let record = DeployRecord {
        id: deploy_id,
        provider: Some(CloudProviderType::Azure),
        droplet_id: 0, // Not applicable for Azure
        instance_id: Some(instance_info.id.clone()),
        hostname: hostname.to_string(),
        ip_address: ip.clone(),
        region: region.to_string(),
        size: size.to_string(),
        ssh_key_path: keypair.private_key_path.display().to_string(),
        ssh_key_fingerprint: String::new(),
        ssh_key_id: None,
        resource_group: Some(resource_group),
        backup_restored,
        created_at: Utc::now(),
    };
    let record_path = record.save()?;
    progress::emit(tx, &format!("  Saved: {}", record_path.display()));
    progress::emit(tx, "\n[Step 16/16] Done!");
    progress::emit(tx, "\n[🚀 Done!] Azure deployment complete!");
    ui::print_summary(&record);
    Ok(record)
}
