use anyhow::{bail, Context, Result};
use chrono::Utc;
use clawmacdo_cloud::digitalocean::DoClient;
use clawmacdo_core::config::{self, CloudProviderType, DeployRecord};
use clawmacdo_db as db;
use clawmacdo_ssh as ssh;
use clawmacdo_ui::progress;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub struct DoRestoreParams {
    pub do_token: String,
    pub snapshot_name: String,
    pub region: Option<String>,
    pub size: Option<String>,
    pub progress_tx: Option<mpsc::UnboundedSender<String>>,
    pub db: Option<Arc<Mutex<rusqlite::Connection>>>,
    pub op_id: Option<String>,
}

/// Result returned on successful restore so callers (serve.rs) can relay details.
pub struct RestoreResult {
    pub deploy_id: String,
    pub hostname: String,
    pub ip_address: String,
    pub ssh_key_path: String,
}

pub async fn run(params: DoRestoreParams) -> Result<RestoreResult> {
    config::ensure_dirs()?;

    let deploy_id = params
        .op_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let hostname = format!("openclaw-{}", &deploy_id[..8]);
    let tx = &params.progress_tx;
    let pdb = &params.db;
    let total: i32 = 5;

    // Step 1
    progress::emit(tx, &format!("\n[Step 1/{total}] Resolving parameters..."));
    db::record_step_start(pdb, &deploy_id, 1, total, "Resolving parameters");
    let region = params
        .region
        .unwrap_or_else(|| config::DEFAULT_REGION.to_string());
    let size = params
        .size
        .unwrap_or_else(|| config::DEFAULT_SIZE.to_string());
    progress::emit(tx, &format!("  Hostname: {hostname}"));
    progress::emit(tx, &format!("  Region:   {region}"));
    progress::emit(tx, &format!("  Size:     {size}"));
    progress::emit(tx, &format!("  Snapshot: {}", params.snapshot_name));
    db::record_step_complete(pdb, &deploy_id, 1);

    // Step 2
    progress::emit(
        tx,
        &format!("\n[Step 2/{total}] Generating SSH key pair..."),
    );
    db::record_step_start(pdb, &deploy_id, 2, total, "Generating SSH key pair");
    let keypair = ssh::generate_keypair(&deploy_id)?;
    progress::emit(
        tx,
        &format!("  Key saved: {}", keypair.private_key_path.display()),
    );
    db::record_step_complete(pdb, &deploy_id, 2);

    // Step 3
    progress::emit(
        tx,
        &format!("\n[Step 3/{total}] Uploading SSH key and looking up snapshot..."),
    );
    db::record_step_start(
        pdb,
        &deploy_id,
        3,
        total,
        "Uploading SSH key & looking up snapshot",
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

    let snapshots = do_client
        .list_snapshots()
        .await
        .context("Failed to list snapshots")?;

    let snapshot = snapshots
        .iter()
        .find(|s| s.name == params.snapshot_name)
        .ok_or_else(|| {
            let available: Vec<&str> = snapshots.iter().map(|s| s.name.as_str()).collect();
            anyhow::anyhow!(
                "Snapshot '{}' not found. Available snapshots: {:?}",
                params.snapshot_name,
                available
            )
        })?;

    let snapshot_id: u64 = snapshot.id.parse().context(format!(
        "Snapshot ID '{}' is not a valid number",
        snapshot.id
    ))?;
    progress::emit(
        tx,
        &format!("  Snapshot found: ID {snapshot_id} ({})", snapshot.name),
    );

    if !snapshot.regions.contains(&region) {
        db::record_step_failed(pdb, &deploy_id, 3, "Snapshot not in region");
        bail!(
            "Snapshot '{}' is not available in region '{region}'. Available regions: {:?}",
            params.snapshot_name,
            snapshot.regions
        );
    }
    db::record_step_complete(pdb, &deploy_id, 3);

    // Step 4
    progress::emit(
        tx,
        &format!("\n[Step 4/{total}] Creating droplet from snapshot..."),
    );
    db::record_step_start(pdb, &deploy_id, 4, total, "Creating droplet from snapshot");
    let droplet = do_client
        .create_droplet_from_snapshot(
            &hostname,
            &region,
            &size,
            snapshot_id,
            key_info.id,
            false,
            "",
        )
        .await
        .context("Failed to create droplet from snapshot")?;
    let droplet_id = droplet.id;
    progress::emit(tx, &format!("  Droplet created: ID {droplet_id}"));
    db::record_step_complete(pdb, &deploy_id, 4);

    // Step 5
    progress::emit(
        tx,
        &format!("\n[Step 5/{total}] Waiting for droplet to become active..."),
    );
    db::record_step_start(
        pdb,
        &deploy_id,
        5,
        total,
        "Waiting for droplet to become active",
    );
    let active_droplet = do_client
        .wait_for_active(droplet_id, std::time::Duration::from_secs(300))
        .await
        .context("Droplet did not become active in time")?;

    let ip = active_droplet
        .public_ip()
        .unwrap_or_else(|| "unknown".into());
    progress::emit(tx, &format!("  Droplet active: {ip}"));
    db::record_step_complete(pdb, &deploy_id, 5);

    // Save to SQLite deployments database
    let conn = db::init_db().context("Failed to open deployments database")?;
    db::insert_deployment(
        &conn,
        &deploy_id,
        "snapshot-restore",
        "",
        "digitalocean",
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
        provider: Some(CloudProviderType::DigitalOcean),
        droplet_id,
        instance_id: None,
        hostname: hostname.clone(),
        ip_address: ip.clone(),
        region: region.clone(),
        size: size.clone(),
        ssh_key_path: keypair.private_key_path.display().to_string(),
        ssh_key_fingerprint: key_info.fingerprint,
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
