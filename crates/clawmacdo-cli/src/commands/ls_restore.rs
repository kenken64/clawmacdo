use anyhow::{Context, Result};
use chrono::Utc;
use clawmacdo_cloud::lightsail_cli::LightsailCliProvider;
use clawmacdo_cloud::CloudProvider;
use clawmacdo_core::config::{self, CloudProviderType, DeployRecord};
use clawmacdo_db as db;
use clawmacdo_provision::provision::commands::ssh_as_openclaw_with_user_async;
use clawmacdo_ssh as ssh;
use clawmacdo_ui::progress;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

pub struct LsRestoreParams {
    pub snapshot_name: String,
    pub region: String,
    pub size: Option<String>,
    pub progress_tx: Option<mpsc::UnboundedSender<String>>,
    pub db: Option<Arc<Mutex<rusqlite::Connection>>>,
    pub op_id: Option<String>,
}

/// Result returned on successful restore.
pub struct RestoreResult {
    pub deploy_id: String,
    pub hostname: String,
    pub ip_address: String,
    pub ssh_key_path: String,
}

fn build_post_restore_repair_cmd() -> String {
    let home = config::OPENCLAW_HOME;
    let mut cmd = r#"export HOME="__HOME__"
export PATH="__HOME__/.local/bin:__HOME__/.local/share/pnpm:/usr/local/bin:/usr/bin:/bin:$PATH"
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus
if [ ! -S "$XDG_RUNTIME_DIR/bus" ]; then
  dbus-daemon --session --address="$DBUS_SESSION_BUS_ADDRESS" --fork >/dev/null 2>&1 || true
fi

node <<'NODE'
const fs = require('fs');
const path = require('path');
const home = process.env.HOME || '__HOME__';
const configPath = path.join(home, '.openclaw', 'openclaw.json');

let changed = false;
try {
  const cfg = JSON.parse(fs.readFileSync(configPath, 'utf8'));
  const telegram = cfg.channels && cfg.channels.telegram;
  if (telegram && telegram.streaming !== undefined) {
    const streaming = telegram.streaming;
    if (!streaming || typeof streaming !== 'object' || Array.isArray(streaming)) {
      delete telegram.streaming;
      changed = true;
    }
  }
  if (changed) {
    fs.copyFileSync(configPath, configPath + '.clawmacdo-restore.bak');
    fs.writeFileSync(configPath, JSON.stringify(cfg, null, 2) + '\n', { mode: 0o600 });
    fs.chmodSync(configPath, 0o600);
  }
} catch (_) {}
console.log(changed ? 'telegram config: normalized legacy streaming value' : 'telegram config: ok');
NODE

(openclaw doctor --fix >/dev/null 2>&1 || true)
(systemctl --user daemon-reload 2>/dev/null || true)
(systemctl --user restart openclaw-gateway.service >/dev/null 2>&1 || \
 systemctl --user start openclaw-gateway.service >/dev/null 2>&1 || true)
for i in $(seq 1 45); do
  if curl -fsS --max-time 2 http://127.0.0.1:18789/health >/dev/null 2>&1; then
    echo 'gateway: healthy'
    exit 0
  fi
  sleep $(( i < 10 ? 2 : 4 ))
done
echo 'gateway: FAILED - not healthy after restore repair'
exit 1
"#
    .to_string();
    cmd = cmd.replace("__HOME__", home);
    cmd
}

pub async fn run(params: LsRestoreParams) -> Result<RestoreResult> {
    config::ensure_dirs()?;

    let deploy_id = params
        .op_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let hostname = format!("openclaw-{}", &deploy_id[..8]);
    let region = params.region;
    let size = params
        .size
        .unwrap_or_else(|| config::DEFAULT_SIZE.to_string());
    let tx = &params.progress_tx;
    let pdb = &params.db;
    let total: i32 = 6;

    let provider = LightsailCliProvider::new(region.clone());
    let bundle_id = provider.get_bundle_id(&size);

    // Step 1/5: Resolve parameters
    progress::emit(tx, &format!("\n[Step 1/{total}] Resolving parameters..."));
    db::record_step_start(pdb, &deploy_id, 1, total, "Resolving parameters");
    progress::emit(tx, &format!("  Hostname: {hostname}"));
    progress::emit(tx, &format!("  Region:   {region}"));
    progress::emit(tx, &format!("  Size:     {size} (bundle: {bundle_id})"));
    progress::emit(tx, &format!("  Snapshot: {}", params.snapshot_name));
    db::record_step_complete(pdb, &deploy_id, 1);

    // Step 2/5: Verify snapshot exists
    progress::emit(tx, &format!("\n[Step 2/{total}] Looking up snapshot..."));
    db::record_step_start(pdb, &deploy_id, 2, total, "Looking up snapshot");
    let snap = provider
        .get_snapshot(&params.snapshot_name)
        .context("Snapshot not found")?;
    progress::emit(
        tx,
        &format!(
            "  Snapshot found: {} ({} GB, from: {})",
            snap.name.as_deref().unwrap_or("?"),
            snap.size_in_gb.unwrap_or(0),
            snap.from_instance_name.as_deref().unwrap_or("?")
        ),
    );
    db::record_step_complete(pdb, &deploy_id, 2);

    // Step 3/5: Generate SSH key pair and upload
    progress::emit(
        tx,
        &format!("\n[Step 3/{total}] Generating and uploading SSH key..."),
    );
    db::record_step_start(
        pdb,
        &deploy_id,
        3,
        total,
        "Generating and uploading SSH key",
    );
    let keypair = ssh::generate_keypair(&deploy_id)?;
    progress::emit(
        tx,
        &format!("  Key saved: {}", keypair.private_key_path.display()),
    );

    let key_name = format!("clawmacdo-{}", &deploy_id[..8]);
    let key_info = provider
        .upload_ssh_key(&key_name, &keypair.public_key_openssh)
        .await
        .context("Failed to upload SSH key")?;
    progress::emit(tx, &format!("  Key uploaded: {}", key_info.id));
    db::record_step_complete(pdb, &deploy_id, 3);

    // Step 4/5: Create instance from snapshot
    progress::emit(
        tx,
        &format!("\n[Step 4/{total}] Creating instance from snapshot..."),
    );
    db::record_step_start(pdb, &deploy_id, 4, total, "Creating instance from snapshot");
    provider
        .create_instance_from_snapshot(&hostname, &params.snapshot_name, &bundle_id, &key_name)
        .context("Failed to create instance from snapshot")?;
    progress::emit(tx, &format!("  Instance creation started: {hostname}"));
    db::record_step_complete(pdb, &deploy_id, 4);

    // Step 5/6: Wait for instance to become active
    progress::emit(
        tx,
        &format!("\n[Step 5/{total}] Waiting for instance to become active..."),
    );
    db::record_step_start(
        pdb,
        &deploy_id,
        5,
        total,
        "Waiting for instance to become active",
    );
    let instance = provider
        .wait_for_active(&hostname, 300)
        .await
        .context("Instance did not become active in time")?;

    let ip = instance.public_ip.unwrap_or_else(|| "unknown".into());
    progress::emit(tx, &format!("  Instance active: {ip}"));
    db::record_step_complete(pdb, &deploy_id, 5);

    // Step 6/6: Repair restored config and restart gateway.
    progress::emit(
        tx,
        &format!("\n[Step 6/{total}] Repairing restored OpenClaw gateway..."),
    );
    db::record_step_start(
        pdb,
        &deploy_id,
        6,
        total,
        "Repairing restored OpenClaw gateway",
    );
    let repair_result: Result<String> = async {
        ssh::wait_for_ssh(
            &ip,
            &keypair.private_key_path,
            Duration::from_secs(300),
            Some("ubuntu"),
        )
        .await
        .context("SSH did not become ready for post-restore repair")?;
        ssh_as_openclaw_with_user_async(
            &ip,
            &keypair.private_key_path,
            &build_post_restore_repair_cmd(),
            "ubuntu",
        )
        .await
        .context("Post-restore gateway repair failed")
    }
    .await;
    match repair_result {
        Ok(output) => {
            let trimmed = output.trim();
            if !trimmed.is_empty() {
                for line in trimmed.lines() {
                    progress::emit(tx, &format!("  {line}"));
                }
            }
            db::record_step_complete(pdb, &deploy_id, 6);
        }
        Err(err) => {
            let msg = format!("Post-restore gateway repair warning: {err:#}");
            progress::emit(tx, &format!("  {msg}"));
            db::record_step_failed(pdb, &deploy_id, 6, &msg);
        }
    }

    // Save to SQLite
    let conn = db::init_db().context("Failed to open deployments database")?;
    db::insert_deployment(
        &conn,
        &deploy_id,
        "snapshot-restore",
        "",
        "lightsail",
        &region,
        &size,
        &hostname,
    )
    .context("Failed to insert deployment record")?;
    db::update_deployment_status(&conn, &deploy_id, "completed", Some(&ip), Some(&hostname))
        .context("Failed to update deployment status")?;

    // Save JSON deploy record
    let record = DeployRecord {
        id: deploy_id.clone(),
        provider: Some(CloudProviderType::Lightsail),
        droplet_id: 0,
        instance_id: None,
        hostname: hostname.clone(),
        ip_address: ip.clone(),
        region: region.clone(),
        size: size.clone(),
        ssh_key_path: keypair.private_key_path.display().to_string(),
        ssh_key_fingerprint: key_info.fingerprint.unwrap_or_default(),
        ssh_key_id: None,
        resource_group: None,
        backup_restored: None,
        created_at: Utc::now(),
    };
    let record_path = record.save()?;

    progress::emit(tx, "\n--- Restore Complete ---");
    progress::emit(tx, &format!("  Deploy ID:   {deploy_id}"));
    progress::emit(tx, &format!("  Hostname:    {hostname}"));
    progress::emit(tx, &format!("  IP Address:  {ip}"));
    progress::emit(tx, &format!("  Snapshot:    {}", params.snapshot_name));
    progress::emit(
        tx,
        &format!("  SSH Key:     {}", keypair.private_key_path.display()),
    );
    progress::emit(tx, &format!("  Record:      {}", record_path.display()));

    Ok(RestoreResult {
        deploy_id,
        hostname,
        ip_address: ip,
        ssh_key_path: keypair.private_key_path.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_restore_repair_normalizes_legacy_telegram_streaming_and_waits_for_health() {
        let cmd = build_post_restore_repair_cmd();
        assert!(cmd.contains("delete telegram.streaming"));
        assert!(cmd.contains("openclaw doctor --fix"));
        assert!(cmd.contains("http://127.0.0.1:18789/health"));
    }
}
