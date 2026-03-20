use anyhow::{Context, Result};
use chrono::Utc;
use clawmacdo_cloud::byteplus::BytePlusClient;
use clawmacdo_core::config::{self, CloudProviderType, DeployRecord};
use clawmacdo_db as db;
use clawmacdo_ssh as ssh;
use clawmacdo_ui::{progress, spinner};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub struct BpRestoreParams {
    pub access_key: String,
    pub secret_key: String,
    pub snapshot_name: String,
    pub region: String,
    pub size: Option<String>,
    pub spot: bool,
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

pub async fn run(params: BpRestoreParams) -> Result<RestoreResult> {
    config::ensure_dirs()?;

    let deploy_id = params
        .op_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let hostname = format!("openclaw-{}", &deploy_id[..8]);
    let region = params.region;
    let size = params
        .size
        .unwrap_or_else(|| config::DEFAULT_BYTEPLUS_SIZE.to_string());
    let tx = &params.progress_tx;
    let pdb = &params.db;
    let total: i32 = 7;

    // Step 1/7: Resolve parameters
    progress::emit(tx, &format!("\n[Step 1/{total}] Resolving parameters..."));
    db::record_step_start(pdb, &deploy_id, 1, total, "Resolving parameters");
    progress::emit(tx, &format!("  Hostname: {hostname}"));
    progress::emit(tx, &format!("  Region:   {region}"));
    progress::emit(tx, &format!("  Size:     {size}"));
    progress::emit(tx, &format!("  Snapshot: {}", params.snapshot_name));
    if params.spot {
        progress::emit(tx, "  Spot:     enabled (SpotAsPriceGo)");
    }
    db::record_step_complete(pdb, &deploy_id, 1);

    let bp_client = BytePlusClient::new(&params.access_key, &params.secret_key, &region)?;

    // Step 2/7: Find snapshot by name
    progress::emit(tx, &format!("\n[Step 2/{total}] Looking up snapshot..."));
    db::record_step_start(pdb, &deploy_id, 2, total, "Looking up snapshot");
    let snapshots = bp_client
        .describe_snapshots(Some(&params.snapshot_name))
        .await
        .context("Failed to list snapshots")?;

    let snapshot = snapshots.first().ok_or_else(|| {
        let available: Vec<&str> = snapshots
            .iter()
            .filter_map(|s| s["SnapshotName"].as_str())
            .collect();
        anyhow::anyhow!(
            "Snapshot '{}' not found. Available snapshots: {:?}",
            params.snapshot_name,
            available
        )
    })?;

    let snapshot_id = snapshot["SnapshotId"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing SnapshotId in snapshot"))?;
    progress::emit(tx, &format!("  Snapshot found: {snapshot_id}"));
    db::record_step_complete(pdb, &deploy_id, 2);

    // Step 3/7: Create custom image from snapshot
    let image_name = format!("openclaw-img-{}", &deploy_id[..8]);
    progress::emit(
        tx,
        &format!("\n[Step 3/{total}] Creating image from snapshot..."),
    );
    db::record_step_start(pdb, &deploy_id, 3, total, "Creating image from snapshot");
    let sp = spinner("Creating image from snapshot...");
    let image_id = bp_client
        .create_image(snapshot_id, &image_name)
        .await
        .context("Failed to create image from snapshot")?;
    progress::emit(tx, &format!("  Image ID: {image_id}"));

    bp_client
        .wait_for_image(&image_id, std::time::Duration::from_secs(600))
        .await
        .context("Image did not become available in time (10 min timeout)")?;
    sp.finish_with_message("Image available.");
    progress::emit(tx, "  Image available.");
    db::record_step_complete(pdb, &deploy_id, 3);

    // Step 4/7: Generate SSH key pair
    progress::emit(
        tx,
        &format!("\n[Step 4/{total}] Generating SSH key pair..."),
    );
    db::record_step_start(pdb, &deploy_id, 4, total, "Generating SSH key pair");
    let keypair = ssh::generate_keypair(&deploy_id)?;
    progress::emit(
        tx,
        &format!("  Key saved: {}", keypair.private_key_path.display()),
    );
    db::record_step_complete(pdb, &deploy_id, 4);

    // Step 5/7: Upload SSH key to BytePlus
    progress::emit(tx, &format!("\n[Step 5/{total}] Uploading SSH key..."));
    db::record_step_start(pdb, &deploy_id, 5, total, "Uploading SSH key");
    let key_name = format!("clawmacdo-{}", &deploy_id[..8]);
    let key_info = bp_client
        .import_key_pair(&key_name, &keypair.public_key_openssh)
        .await
        .context("Failed to upload SSH key")?;
    progress::emit(tx, &format!("  Key ID: {}", key_info.id));
    db::record_step_complete(pdb, &deploy_id, 5);

    // Step 6/7: Create instance from custom image
    progress::emit(
        tx,
        &format!("\n[Step 6/{total}] Creating instance from image..."),
    );
    db::record_step_start(pdb, &deploy_id, 6, total, "Creating instance from image");
    let instance_id = bp_client
        .create_instance_from_image(&hostname, &image_id, &size, &key_name, "", params.spot)
        .await
        .context("Failed to create instance from image")?;
    progress::emit(tx, &format!("  Instance created: {instance_id}"));
    db::record_step_complete(pdb, &deploy_id, 6);

    // Step 7/7: Wait for instance to become RUNNING
    progress::emit(
        tx,
        &format!("\n[Step 7/{total}] Waiting for instance to become active..."),
    );
    db::record_step_start(
        pdb,
        &deploy_id,
        7,
        total,
        "Waiting for instance to become active",
    );
    let sp = spinner("Waiting for instance...");
    let instance = bp_client
        .wait_for_running(&instance_id, std::time::Duration::from_secs(300))
        .await
        .context("Instance did not become RUNNING in time")?;

    let ip = instance.public_ip.unwrap_or_else(|| "unknown".into());
    sp.finish_with_message(format!("Instance active at {ip}"));
    progress::emit(tx, &format!("  Instance active: {ip}"));
    db::record_step_complete(pdb, &deploy_id, 7);

    // Save to SQLite
    let conn = db::init_db().context("Failed to open deployments database")?;
    db::insert_deployment(
        &conn,
        &deploy_id,
        "snapshot-restore",
        "",
        "byteplus",
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
        provider: Some(CloudProviderType::BytePlus),
        droplet_id: 0,
        instance_id: Some(instance_id),
        hostname: hostname.clone(),
        ip_address: ip.clone(),
        region: region.clone(),
        size: size.clone(),
        ssh_key_path: keypair.private_key_path.display().to_string(),
        ssh_key_fingerprint: key_info.fingerprint.unwrap_or_default(),
        ssh_key_id: Some(key_info.id),
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
    progress::emit(tx, &format!("  Image:       {image_id}"));
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
