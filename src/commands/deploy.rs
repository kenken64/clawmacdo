use crate::config::{self, DeployRecord};
use crate::digitalocean::DoClient;
use crate::{cloud_init, ssh, ui};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::path::PathBuf;

/// Parameters for a deploy operation.
pub struct DeployParams {
    pub do_token: String,
    pub anthropic_key: String,
    pub openai_key: String,
    pub region: Option<String>,
    pub size: Option<String>,
    pub hostname: Option<String>,
    pub backup: Option<PathBuf>,
    pub enable_backups: bool,
    /// If true, skip interactive prompts and use the pre-set backup path.
    pub non_interactive: bool,
}

/// Run the full 12-step deploy flow. Returns the DeployRecord on success.
pub async fn run(params: DeployParams) -> Result<DeployRecord> {
    config::ensure_dirs()?;
    let deploy_id = uuid::Uuid::new_v4().to_string();

    // ── Step 1: Resolve parameters ──────────────────────────────────────
    println!("\n[Step 1/12] Resolving parameters...");

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

    println!("  Region:   {region}");
    println!("  Size:     {size}");
    println!("  Hostname: {hostname}");
    println!(
        "  Backup:   {}",
        backup_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "None".into())
    );

    // ── Step 2: Generate SSH key pair ───────────────────────────────────
    println!("\n[Step 2/12] Generating Ed25519 SSH key pair...");

    let keypair = ssh::generate_keypair(&deploy_id)?;
    println!("  Key saved: {}", keypair.private_key_path.display());

    // ── Step 3: Upload public key to DO ─────────────────────────────────
    println!("\n[Step 3/12] Uploading SSH public key to DigitalOcean...");

    let do_client = DoClient::new(&params.do_token)?;
    let key_name = format!("clawmacdo-{}", &deploy_id[..8]);
    let key_info = do_client
        .upload_ssh_key(&key_name, &keypair.public_key_openssh)
        .await
        .context("Failed to upload SSH key to DigitalOcean")?;
    println!(
        "  Key ID: {}, Fingerprint: {}",
        key_info.id, key_info.fingerprint
    );

    // ── Step 4: Create droplet ──────────────────────────────────────────
    println!("\n[Step 4/12] Creating droplet with cloud-init...");

    let user_data = cloud_init::generate(&params.anthropic_key, &params.openai_key);
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
    println!("  Droplet created: ID {droplet_id}");

    // From here on, if we fail we print debug info instead of auto-destroying.
    let result = deploy_steps_5_through_12(
        &do_client,
        droplet_id,
        &keypair.private_key_path,
        &key_info.fingerprint,
        backup_path.as_deref(),
        &deploy_id,
        &hostname,
        &region,
        &size,
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

async fn deploy_steps_5_through_12(
    do_client: &DoClient,
    droplet_id: u64,
    private_key_path: &std::path::Path,
    ssh_fingerprint: &str,
    backup_path: Option<&std::path::Path>,
    deploy_id: &str,
    hostname: &str,
    region: &str,
    size: &str,
) -> Result<DeployRecord> {
    // ── Step 5: Poll until droplet is active ────────────────────────────
    let sp = ui::spinner("[Step 5/12] Waiting for droplet to become active...");
    let droplet = do_client
        .wait_for_active(droplet_id, std::time::Duration::from_secs(300))
        .await
        .context("Droplet did not become active within 5 minutes")?;
    let ip = droplet.public_ip().unwrap();
    sp.finish_with_message(format!("[Step 5/12] Droplet active at {ip}"));

    // ── Step 6: Wait for SSH ────────────────────────────────────────────
    let sp = ui::spinner("[Step 6/12] Waiting for SSH...");
    ssh::wait_for_ssh(&ip, private_key_path, std::time::Duration::from_secs(120))
        .await
        .context("SSH did not become available within 2 minutes")?;
    sp.finish_with_message("[Step 6/12] SSH ready");

    // ── Step 7: Wait for cloud-init ─────────────────────────────────────
    let sp = ui::spinner(
        "[Step 7/12] Waiting for cloud-init to finish (this may take a few minutes)...",
    );
    ssh::wait_for_cloud_init(&ip, private_key_path, std::time::Duration::from_secs(600))
        .await
        .context("Cloud-init did not complete within 10 minutes")?;
    sp.finish_with_message("[Step 7/12] Cloud-init complete");

    // ── Step 8: SCP backup archive ──────────────────────────────────────
    let backup_restored: Option<String>;
    if let Some(bp) = backup_path {
        let sp = ui::spinner("[Step 8/12] Uploading backup archive...");
        let remote_archive = "/tmp/openclaw_backup.tar.gz";
        let ip_clone = ip.clone();
        let key_clone = private_key_path.to_path_buf();
        let bp_clone = bp.to_path_buf();
        tokio::task::spawn_blocking(move || {
            ssh::scp_upload(&ip_clone, &key_clone, &bp_clone, remote_archive)
        })
        .await??;
        sp.finish_with_message("[Step 8/12] Backup uploaded");
        backup_restored = Some(bp.display().to_string());

        // ── Step 9: Extract configs ─────────────────────────────────────
        let sp = ui::spinner("[Step 9/12] Extracting backup on server...");
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
        sp.finish_with_message("[Step 9/12] Backup restored (preserved .env)");
    } else {
        println!("[Step 8/12] No backup to upload, skipping.");
        println!("[Step 9/12] No backup to extract, skipping.");
        backup_restored = None;
    }

    // ── Step 10: Start gateway ──────────────────────────────────────────
    let sp = ui::spinner("[Step 10/12] Starting OpenClaw gateway...");
    let start_cmd = concat!(
        "openclaw onboard --install-daemon 2>/dev/null; ",
        "systemctl enable openclaw-gateway && ",
        "systemctl start openclaw-gateway && ",
        "echo ok"
    );
    let ip_clone = ip.clone();
    let key_clone = private_key_path.to_path_buf();
    tokio::task::spawn_blocking(move || ssh::exec(&ip_clone, &key_clone, start_cmd)).await??;
    sp.finish_with_message("[Step 10/12] Gateway started");

    // ── Step 11: Save DeployRecord ──────────────────────────────────────
    println!("[Step 11/12] Saving deploy record...");
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
    println!("  Saved: {}", record_path.display());

    // ── Step 12: Print summary ──────────────────────────────────────────
    println!("[Step 12/12] Done!");
    ui::print_summary(&record);

    Ok(record)
}
