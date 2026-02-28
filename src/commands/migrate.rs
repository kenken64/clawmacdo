use crate::commands::deploy::{self, DeployParams};
use crate::{config, ssh, ui};
use anyhow::{Context, Result};
use chrono::Utc;
use std::path::PathBuf;

/// Parameters for a DO → DO migration.
pub struct MigrateParams {
    pub do_token: String,
    pub anthropic_key: String,
    pub openai_key: String,
    pub gemini_key: String,
    pub whatsapp_phone_number: String,
    pub telegram_bot_token: String,
    pub source_ip: String,
    pub source_key: PathBuf,
    pub region: Option<String>,
    pub size: Option<String>,
    pub hostname: Option<String>,
}

/// Run the full migrate flow: remote backup from source, then deploy to new droplet.
pub async fn run(params: MigrateParams) -> Result<()> {
    config::ensure_dirs()?;

    // ── Step 1: SSH into source droplet ─────────────────────────────────
    let sp = ui::spinner("[Migrate 1/5] Connecting to source droplet...");
    let source_ip = params.source_ip.clone();
    let source_key = params.source_key.clone();
    let verify_ip = source_ip.clone();
    let verify_key = source_key.clone();
    tokio::task::spawn_blocking(move || ssh::exec(&verify_ip, &verify_key, "echo ok"))
        .await?
        .context("Failed to connect to source droplet")?;
    sp.finish_with_message("[Migrate 1/5] Connected to source");

    // ── Step 2: Remote backup on source ─────────────────────────────────
    let sp = ui::spinner("[Migrate 2/5] Creating backup on source droplet...");
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let remote_archive = format!("/tmp/openclaw_migrate_{timestamp}.tar.gz");
    let tar_cmd = format!("cd /root && tar czf {remote_archive} .openclaw/ 2>/dev/null && echo ok");
    let tar_ip = source_ip.clone();
    let tar_key = source_key.clone();
    tokio::task::spawn_blocking(move || ssh::exec(&tar_ip, &tar_key, &tar_cmd))
        .await?
        .context("Failed to create remote backup")?;
    sp.finish_with_message("[Migrate 2/5] Remote backup created");

    // ── Step 3: SCP download backup to local ────────────────────────────
    let sp = ui::spinner("[Migrate 3/5] Downloading backup from source...");
    let local_archive_name = format!("openclaw_migrate_{timestamp}.tar.gz");
    let local_archive = config::backups_dir()?.join(&local_archive_name);
    let dl_ip = source_ip.clone();
    let dl_key = source_key.clone();
    let dl_remote = remote_archive.clone();
    let dl_local = local_archive.clone();
    tokio::task::spawn_blocking(move || ssh::scp_download(&dl_ip, &dl_key, &dl_remote, &dl_local))
        .await?
        .context("Failed to download backup from source")?;
    sp.finish_with_message(format!(
        "[Migrate 3/5] Backup downloaded: {}",
        local_archive.display()
    ));

    // ── Step 4: Run full deploy with the downloaded backup ──────────────
    println!("[Migrate 4/5] Starting deploy to new droplet...");
    let deploy_params = DeployParams {
        do_token: params.do_token,
        anthropic_key: params.anthropic_key,
        openai_key: params.openai_key,
        gemini_key: params.gemini_key,
        whatsapp_phone_number: params.whatsapp_phone_number,
        telegram_bot_token: params.telegram_bot_token,
        region: params.region,
        size: params.size,
        hostname: params.hostname,
        backup: Some(local_archive),
        enable_backups: false,
        non_interactive: false,
        progress_tx: None,
    };
    let record = deploy::run(deploy_params).await?;

    // ── Step 5: Print migration summary ─────────────────────────────────
    ui::print_migrate_summary(&source_ip, &record);

    Ok(())
}
