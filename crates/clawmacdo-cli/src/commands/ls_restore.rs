use anyhow::{Context, Result};
use chrono::Utc;
use clawmacdo_cloud::lightsail_cli::LightsailCliProvider;
use clawmacdo_cloud::CloudProvider;
use clawmacdo_core::config::{self, CloudProviderType, DeployRecord};
use clawmacdo_db as db;
use clawmacdo_ssh as ssh;
use clawmacdo_ui::progress;
use std::sync::{Arc, Mutex};
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
    let total: i32 = 5;

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

    // Step 5/5: Wait for instance to become active
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
